//! `canvas bridge host`: the broker process.
//!
//! Chrome starts it with the caller origin as the first argument and speaks
//! length-prefixed JSON on the pipes (REPORT §3.4). It is the **broker owner**
//! for exactly one identity:
//!
//! - it holds the shared identity lock (§10) for its lifetime, and answers
//!   `identity remove`'s cooperative release by detaching and exiting;
//! - it holds `<data root>/bridge/<key>.lock` exclusively, and a second host
//!   for the same identity reports the existing owner and exits;
//! - it serves `bridge-ipc@1` on `<data root>/bridge/<key>.sock`, mode `0600`
//!   inside a `0700` directory.
//!
//! This process never speaks the §7 output contract: stdout is the native
//! messaging channel, so everything a person should read goes to stderr.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use canvas_core::bridge::framing;
use canvas_core::bridge::ipc::{self, Body, Op, Reason, Response};
use canvas_core::bridge::state::{Accepted, Broker, OwnedIdentity};
use canvas_core::bridge::wire::{
    ExtensionMessage, HostMessage, NATIVE_PROTOCOL, Observation, PauseCause,
};
use canvas_core::bridge::{Endpoint, state};
use canvas_core::identity::{IdentityDocument, IdentityLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use crate::bridge::manifest;
use crate::bridge::owner::{OwnerError, OwnerRecord, Ownership};
use crate::commands::Globals;

/// How often the host re-reads `identity.json` (§10).
const IDENTITY_POLL: Duration = Duration::from_secs(2);

/// How long a consumer waits for the extension to re-probe and extract.
const TEXT_TIMEOUT: Duration = Duration::from_secs(5);

/// Exit codes this process uses. It has no §7 envelope, so these are the
/// §14 codes it can still express honestly.
const EXIT_REFUSED: u8 = 8;
const EXIT_LOCAL: u8 = 13;

/// A pending text request, waiting for the extension's answer.
type TextWaiters =
    Arc<Mutex<std::collections::HashMap<String, oneshot::Sender<Result<(), Reason>>>>>;

/// The shared parts of one running host.
struct Host {
    broker: Arc<tokio::sync::Mutex<Broker>>,
    /// The native-messaging channel to Chrome, shared by every task.
    stdout: Arc<Mutex<io::Stdout>>,
    waiters: TextWaiters,
    /// Set when the host must stop: released, disconnected, or replaced.
    stop: Arc<tokio::sync::Notify>,
    stopping: Arc<std::sync::atomic::AtomicBool>,
}

impl Host {
    /// Send one message to the extension.
    fn send(&self, message: &HostMessage) {
        let mut stdout = match self.stdout.lock() {
            Ok(stdout) => stdout,
            Err(poisoned) => poisoned.into_inner(),
        };
        if framing::write_json(&mut *stdout, message).is_err() {
            // The pipe is gone: nothing more can be shared.
            self.shutdown();
        }
    }

    fn shutdown(&self) {
        self.stopping
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.stop.notify_waiters();
    }
}

/// Run the broker. `caller_origin` is Chrome's first argument.
pub async fn run(globals: &Globals, caller_origin: Option<String>) -> ExitCode {
    match serve(globals, caller_origin.as_deref()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(HostError::Refused(message)) => {
            let _ = writeln!(io::stderr(), "{message}");
            ExitCode::from(EXIT_REFUSED)
        }
        Err(HostError::Local(message)) => {
            let _ = writeln!(io::stderr(), "{message}");
            ExitCode::from(EXIT_LOCAL)
        }
    }
}

/// Why the host stopped before it served anything.
enum HostError {
    /// A domain refusal: a wrong caller, or another owner.
    Refused(String),
    /// A local failure: no identity, no lock, no endpoint.
    Local(String),
}

async fn serve(globals: &Globals, caller_origin: Option<&str>) -> Result<(), HostError> {
    let session = globals
        .open_local_session()
        .map_err(|e| HostError::Local(format!("cannot open an identity: {e}")))?;
    let configured = configured_extension_id();

    // The caller is checked before anything is read from the pipe.
    let extension_id = check_caller(caller_origin, configured.as_deref())?;

    // §10: a resident consumer holds the identity lock for its lifetime.
    let identity_lock = IdentityLock::acquire_shared(&session.paths, &session.identity)
        .map_err(|e| HostError::Local(format!("cannot hold the identity: {e}")))?;

    let endpoint = Endpoint::for_identity(&session.paths.data_root, &session.identity.key);
    let record = OwnerRecord {
        pid: std::process::id(),
        started_at: crate::output::generated_at_now(),
        identity_key: session.identity.key.to_string(),
        endpoint: endpoint.address(),
    };
    let ownership = match Ownership::take(&endpoint, &record) {
        Ok(ownership) => ownership,
        Err(OwnerError::Taken(existing)) => {
            let who = existing.as_ref().as_ref().map_or_else(
                || "another process".to_owned(),
                |owner| format!("pid {} since {}", owner.pid, owner.started_at),
            );
            return Err(HostError::Refused(format!(
                "a canvas bridge host already owns {}: {who}. It is reported, never replaced.",
                session.identity.key
            )));
        }
        Err(OwnerError::Io(e)) => {
            return Err(HostError::Local(format!(
                "cannot take broker ownership: {e}"
            )));
        }
    };
    // Under the ownership lock, no live host can be serving the endpoint.
    ownership
        .clear_stale_socket()
        .map_err(|e| HostError::Local(format!("cannot clear a stale endpoint: {e}")))?;

    let identity = OwnedIdentity {
        key: session.identity.key.to_string(),
        origin: session.identity.origin.clone(),
        user_id: session.identity.user_id,
        generation: session.identity.generation.to_string(),
    };
    let broker = Broker::new(identity, Some(extension_id.to_owned()));
    let host = Host {
        broker: Arc::new(tokio::sync::Mutex::new(broker)),
        stdout: Arc::new(Mutex::new(io::stdout())),
        waiters: TextWaiters::default(),
        stop: Arc::new(tokio::sync::Notify::new()),
        stopping: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let host = Arc::new(host);

    let listener = bind(&endpoint)
        .map_err(|e| HostError::Local(format!("cannot serve the broker endpoint: {e}")))?;
    ownership
        .restrict_socket()
        .map_err(|e| HostError::Local(format!("cannot restrict the endpoint: {e}")))?;

    let pause_after = pause_hidden_after_ms();
    host.send(&HostMessage::Ready {
        protocol: NATIVE_PROTOCOL.to_owned(),
        identity_key: session.identity.key.to_string(),
        origin: session.identity.origin.clone(),
        pause_hidden_after_ms: pause_after,
    });

    let messages = read_stdin();
    let identity_json = session.paths.identity_json();
    let generation = session.identity.generation.to_string();
    let outcome = run_loop(&host, listener, messages, &identity_json, &generation).await;

    // Erase everything before the process ends.
    host.broker.lock().await.detach_all();
    let _ = std::fs::remove_file(&endpoint.socket);
    drop(ownership);
    drop(identity_lock);
    outcome
}

/// The main loop: extension messages, consumer connections, and the watchdog.
async fn run_loop(
    host: &Arc<Host>,
    listener: tokio::net::UnixListener,
    mut messages: mpsc::UnboundedReceiver<ExtensionMessage>,
    identity_json: &Path,
    generation: &str,
) -> Result<(), HostError> {
    let mut watchdog = tokio::time::interval(IDENTITY_POLL);
    watchdog.tick().await;
    loop {
        tokio::select! {
            () = host.stop.notified() => return Ok(()),
            message = messages.recv() => {
                let Some(message) = message else {
                    // Chrome closed the pipe: host loss ends sharing.
                    return Ok(());
                };
                handle_extension(host, message).await;
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        let host = Arc::clone(host);
                        tokio::spawn(async move { serve_consumer(&host, stream).await; });
                    }
                    Err(e) => {
                        return Err(HostError::Local(format!("the broker endpoint failed: {e}")));
                    }
                }
            }
            _ = watchdog.tick() => {
                if !still_bound(identity_json, generation) {
                    // §10: the identity is gone or was recreated.
                    return Ok(());
                }
            }
        }
        if host.stopping.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
    }
}

/// Apply one message from the companion.
async fn handle_extension(host: &Arc<Host>, message: ExtensionMessage) {
    match message {
        ExtensionMessage::Hello {
            protocol,
            extension_id,
            profile_instance,
        } => {
            let mut broker = host.broker.lock().await;
            if protocol != NATIVE_PROTOCOL {
                host.send(&HostMessage::Refused {
                    reason: Reason::Protocol.as_str().to_owned(),
                });
                host.shutdown();
                return;
            }
            if let Err(reason) = broker.hello(&extension_id, &profile_instance) {
                host.send(&HostMessage::Refused {
                    reason: reason.as_str().to_owned(),
                });
                host.shutdown();
            }
        }
        ExtensionMessage::Attach { observation, .. } => {
            let mut broker = host.broker.lock().await;
            match broker.attach(&observation) {
                Accepted::Attached { .. } => {
                    let state = broker
                        .list()
                        .first()
                        .map_or("attached", |summary| summary.state.as_str())
                        .to_owned();
                    drop(broker);
                    host.send(&HostMessage::Attached { state });
                }
                Accepted::Refused { reason } => {
                    drop(broker);
                    host.send(&HostMessage::Refused {
                        reason: reason.as_str().to_owned(),
                    });
                }
                _ => {}
            }
        }
        ExtensionMessage::Update { observation } => {
            update(host, &observation).await;
        }
        ExtensionMessage::Text {
            request_id,
            document_id,
            navigation_generation,
            account,
            zone,
            extract,
        } => {
            let outcome = {
                let mut broker = host.broker.lock().await;
                broker.text(
                    &document_id,
                    navigation_generation,
                    account.as_ref(),
                    zone,
                    extract,
                )
            };
            let stored = match outcome {
                Accepted::Updated => Ok(()),
                Accepted::Refused { reason } => Err(reason),
                _ => Err(Reason::NotAttached),
            };
            let waiter = host
                .waiters
                .lock()
                .ok()
                .and_then(|mut waiters| waiters.remove(&request_id));
            if let Some(waiter) = waiter {
                let _ = waiter.send(stored);
            }
        }
        ExtensionMessage::Pause { cause } => {
            host.broker.lock().await.pause(cause);
        }
        ExtensionMessage::Detach { cause } => {
            let mut broker = host.broker.lock().await;
            broker.pause(cause);
            broker.detach_all();
            if cause == PauseCause::CrossOrigin {
                drop(broker);
                host.send(&HostMessage::Detach {
                    reason: "cross_origin".to_owned(),
                });
            }
        }
    }
}

/// Apply one observation and finish validation when the account matched.
async fn update(host: &Arc<Host>, observation: &Observation) {
    let mut broker = host.broker.lock().await;
    match broker.update(observation) {
        Accepted::Updated => {
            // The account probe arrived with the observation, so a document
            // that entered `validating` is validated now.
            broker.validated();
        }
        Accepted::Ended => {
            drop(broker);
            host.send(&HostMessage::Detach {
                reason: "cross_origin".to_owned(),
            });
        }
        Accepted::Refused { reason } => {
            drop(broker);
            host.send(&HostMessage::Refused {
                reason: reason.as_str().to_owned(),
            });
        }
        Accepted::Attached { .. } => {}
    }
}

/// Serve one consumer connection: newline-delimited `bridge-ipc@1`.
async fn serve_consumer(host: &Arc<Host>, stream: tokio::net::UnixStream) {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    loop {
        // The bound is applied while the line is read, not after it exists.
        let line = match read_bounded_line(&mut reader, ipc::MAX_REQUEST_BYTES).await {
            Ok(Some(line)) => line,
            Ok(None) => return,
            Err(reason) => {
                let _ = write_line(&mut write, &Response::refused("", reason)).await;
                return;
            }
        };
        let request = match ipc::parse_request(line.trim()) {
            Ok(request) => request,
            Err(reason) => {
                let _ = write_line(&mut write, &Response::refused("", reason)).await;
                return;
            }
        };
        let id = request.id.clone();
        let response = answer(host, request).await;
        if write_line(&mut write, &Response::ok(id, response))
            .await
            .is_err()
        {
            return;
        }
    }
}

/// Answer one operation.
async fn answer(host: &Arc<Host>, request: ipc::Request) -> Body {
    match request.op {
        Op::AttachmentsList => Body::Attachments {
            attachments: host.broker.lock().await.list(),
        },
        Op::Attach {
            consumer,
            attachment_id,
        } => match host
            .broker
            .lock()
            .await
            .attach_consumer(attachment_id.as_deref(), &consumer)
        {
            Ok((attachment_id, state)) => Body::Attached {
                attachment_id,
                state,
            },
            Err(reason) => Body::Refused { reason },
        },
        Op::Detach {
            attachment_id,
            consumer,
        } => {
            let mut broker = host.broker.lock().await;
            // A named consumer gives up only its own opt-in. Ending the
            // attachment is a human act, so it stays with the CLI.
            let ended = match consumer.as_deref() {
                Some(consumer) => broker.detach_consumer(attachment_id.as_deref(), consumer),
                None => broker.detach(attachment_id.as_deref()),
            };
            let detach_extension = consumer.is_none() && ended.is_ok();
            drop(broker);
            if detach_extension {
                host.send(&HostMessage::Detach {
                    reason: "user_detached".to_owned(),
                });
            }
            match ended {
                Ok(()) => Body::Detached { detached: true },
                Err(reason) => Body::Refused { reason },
            }
        }
        Op::Release { reason } => {
            let _ = writeln!(io::stderr(), "releasing the identity: {reason}");
            host.broker.lock().await.detach_all();
            host.send(&HostMessage::Detach {
                reason: "identity_released".to_owned(),
            });
            host.shutdown();
            Body::Released { released: true }
        }
        Op::Here {
            attachment_id,
            consumer,
            include_text,
        } => here(host, attachment_id, consumer, include_text).await,
    }
}

/// The `here` operation, with the fresh probe a text release requires.
///
/// Metadata is served from the broker's last validated state. Text is
/// released only after the extension re-probed the account for **this**
/// document (REPORT §3.3 step 4), and an account that no longer matches
/// refuses the whole answer rather than returning a bundle without it.
async fn here(
    host: &Arc<Host>,
    attachment_id: Option<String>,
    consumer: Option<String>,
    include_text: bool,
) -> Body {
    let probe = if include_text {
        request_text(host).await.err()
    } else {
        None
    };
    if probe == Some(Reason::AccountMismatch) {
        return Body::Refused {
            reason: Reason::AccountMismatch,
        };
    }
    match host.broker.lock().await.context(
        attachment_id.as_deref(),
        consumer.as_deref(),
        include_text,
    ) {
        Ok(mut context) => {
            // Say why the text is absent when the probe is the reason.
            if context.content_reason.is_none() && context.text.is_none() {
                context.content_reason = probe;
            }
            Body::Context(Box::new(context))
        }
        Err(reason) => Body::Refused { reason },
    }
}

/// Ask the extension to re-probe and extract, and wait for the answer.
async fn request_text(host: &Arc<Host>) -> Result<(), Reason> {
    let Some((document_id, navigation_generation)) = host.broker.lock().await.current_document()
    else {
        return Err(Reason::NotAttached);
    };
    let request_id = state::new_attachment_id();
    let (tx, rx) = oneshot::channel();
    if let Ok(mut waiters) = host.waiters.lock() {
        waiters.insert(request_id.clone(), tx);
    }
    host.send(&HostMessage::RequestText {
        request_id: request_id.clone(),
        document_id,
        navigation_generation,
    });
    match tokio::time::timeout(TEXT_TIMEOUT, rx).await {
        Ok(Ok(stored)) => stored,
        Ok(Err(_)) => Err(Reason::NotAttached),
        Err(_) => {
            if let Ok(mut waiters) = host.waiters.lock() {
                waiters.remove(&request_id);
            }
            // The companion did not answer in time: the document is not in a
            // state this host can vouch for.
            Err(Reason::Validating)
        }
    }
}

async fn write_line(
    write: &mut tokio::net::unix::OwnedWriteHalf,
    response: &Response,
) -> io::Result<()> {
    let mut line = serde_json::to_vec(response).unwrap_or_default();
    line.push(b'\n');
    write.write_all(&line).await?;
    write.flush().await
}

/// Read one newline-terminated request, refusing an oversize one as it
/// arrives rather than after it has been assembled.
///
/// `Ok(None)` is a clean end of stream.
async fn read_bounded_line(
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
    max: usize,
) -> Result<Option<String>, Reason> {
    let mut line = Vec::new();
    loop {
        let Ok(available) = reader.fill_buf().await else {
            return Ok(None);
        };
        if available.is_empty() {
            return Ok(if line.is_empty() {
                None
            } else {
                Some(String::new())
            });
        }
        if let Some(at) = available.iter().position(|b| *b == b'\n') {
            if line.len() + at > max {
                return Err(Reason::Protocol);
            }
            line.extend_from_slice(&available[..at]);
            reader.consume(at + 1);
            return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
        }
        let taken = available.len();
        if line.len() + taken > max {
            return Err(Reason::Protocol);
        }
        line.extend_from_slice(available);
        reader.consume(taken);
    }
}

/// Bind the endpoint after the stale one was cleared.
fn bind(endpoint: &Endpoint) -> io::Result<tokio::net::UnixListener> {
    if !endpoint.path_fits() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} is longer than the {} bytes a Unix socket path may have; \
                 set CANVAS_DATA_ROOT to a shorter directory",
                endpoint.socket.display(),
                canvas_core::bridge::endpoint::MAX_SOCKET_PATH
            ),
        ));
    }
    tokio::net::UnixListener::bind(&endpoint.socket)
}

/// Read framed messages from stdin on a dedicated thread.
///
/// Chrome's channel is a blocking pipe, so it gets a thread of its own rather
/// than a runtime feature that would also expose stdout to the runtime.
fn read_stdin() -> mpsc::UnboundedReceiver<ExtensionMessage> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut lock = stdin.lock();
        loop {
            match framing::read_json::<ExtensionMessage>(&mut lock) {
                Ok(Some(message)) => {
                    if tx.send(message).is_err() {
                        return;
                    }
                }
                // A clean end of stream, or anything this protocol does not
                // define: either way the port is over.
                Ok(None) | Err(_) => return,
            }
        }
    });
    rx
}

/// Whether `identity.json` still describes the bound generation.
fn still_bound(identity_json: &Path, generation: &str) -> bool {
    IdentityDocument::read(identity_json).is_ok_and(|doc| doc.generation.to_string() == generation)
}

/// The extension id `bridge.extension_id` names, when one is configured.
fn configured_extension_id() -> Option<String> {
    crate::paths::CliPaths::resolve()
        .ok()
        .and_then(|paths| crate::config::Config::load_file(&paths).ok())
        .and_then(|config| config.bridge.extension_id)
}

/// `bridge.pause_hidden_after` in milliseconds.
fn pause_hidden_after_ms() -> u64 {
    crate::paths::CliPaths::resolve()
        .ok()
        .and_then(|paths| crate::config::Config::load_file(&paths).ok())
        .map_or(crate::config::DEFAULT_PAUSE_HIDDEN_MS, |config| {
            crate::config::pause_hidden_after_ms(&config.bridge.pause_hidden_after)
        })
}

/// Check Chrome's caller origin against the configured extension id.
///
/// Chrome's `allowed_origins` is the browser's own check; this is the host's,
/// because the manifest is a file another installation could have rewritten.
fn check_caller<'a>(
    caller_origin: Option<&'a str>,
    configured: Option<&'a str>,
) -> Result<&'a str, HostError> {
    let Some(origin) = caller_origin else {
        return Err(HostError::Refused(
            "canvas bridge host is started by Chrome; \
             run `canvas bridge install` and load the companion"
                .to_owned(),
        ));
    };
    let Some(id) = manifest::origin_extension_id(origin) else {
        return Err(HostError::Refused(format!(
            "refusing a caller that is not a Chrome extension origin: {origin}"
        )));
    };
    match configured {
        Some(expected) if expected != id => Err(HostError::Refused(format!(
            "refusing extension {id}: this identity is configured for {expected}"
        ))),
        _ => Ok(id),
    }
}

/// The absolute path of the running `canvas` binary, for the manifest.
pub fn binary_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "abcdefghijklmnopabcdefghijklmnop";

    /// M7-a acceptance: a wrong id or a wrong origin is refused.
    #[test]
    fn only_the_configured_extension_may_start_the_host() {
        let origin = format!("chrome-extension://{ID}/");
        assert_eq!(check_caller(Some(&origin), Some(ID)).ok(), Some(ID));
        assert_eq!(check_caller(Some(&origin), None).ok(), Some(ID));

        let other = "ponmlkjihgfedcbaponmlkjihgfedcba";
        assert!(matches!(
            check_caller(Some(&format!("chrome-extension://{other}/")), Some(ID)),
            Err(HostError::Refused(_))
        ));
        for bad in [
            "https://evil.test/",
            "chrome-extension://short/",
            "moz-extension://abcdefghijklmnopabcdefghijklmnop/",
            "",
        ] {
            assert!(
                matches!(
                    check_caller(Some(bad), Some(ID)),
                    Err(HostError::Refused(_))
                ),
                "{bad}"
            );
        }
        assert!(matches!(
            check_caller(None, Some(ID)),
            Err(HostError::Refused(_))
        ));
    }
}
