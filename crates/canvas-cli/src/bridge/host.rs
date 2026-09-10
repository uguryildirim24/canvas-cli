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
use canvas_core::bridge::ipc::{self, Body, FollowStatus, LoadOutcome, Op, Reason, Response};
use canvas_core::bridge::state::{Accepted, Broker, OwnedIdentity};
use canvas_core::bridge::wire::{
    ExtensionMessage, HostMessage, NATIVE_PROTOCOL, Observation, PanelState, PauseCause,
};
use canvas_core::bridge::{Endpoint, state};
use canvas_core::identity::{IdentityDocument, IdentityLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use crate::bridge::manifest;
use crate::bridge::owner::{OwnerError, OwnerRecord, Ownership};
use crate::bridge::panel::{self, Decision};
use crate::commands::Globals;
use crate::session::Session;

/// How often the host re-reads `identity.json` (§10).
const IDENTITY_POLL: Duration = Duration::from_secs(2);

/// How long a consumer waits for the extension to re-probe and extract.
///
/// `bridge::client::TIMEOUT` is longer than this on purpose, so the client
/// hears this host's own answer rather than giving up first.
pub(crate) const TEXT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a `follow` waits for the companion's dispatch acknowledgement.
///
/// REPORT §3.2 proposes a p95 under 300 ms for the acknowledgement, so a wait
/// several times that is generous and still well inside the client's own
/// timeout. Exceeding it is `navigation_timeout`, not a claim that the page
/// did or did not load.
pub(crate) const NAVIGATE_TIMEOUT: Duration = Duration::from_secs(2);

/// Exit codes this process uses. It has no §7 envelope, so these are the
/// §14 codes it can still express honestly.
const EXIT_REFUSED: u8 = 8;
const EXIT_LOCAL: u8 = 13;

/// A pending text request, waiting for the extension's answer.
type TextWaiters =
    Arc<Mutex<std::collections::HashMap<String, oneshot::Sender<Result<(), Reason>>>>>;

/// A pending navigation, waiting for the companion to acknowledge it.
type NavigateWaiters =
    Arc<Mutex<std::collections::HashMap<String, oneshot::Sender<Result<(), Reason>>>>>;

/// The shared parts of one running host.
struct Host {
    broker: Arc<tokio::sync::Mutex<Broker>>,
    /// The native-messaging channel to Chrome, shared by every task.
    stdout: Arc<Mutex<io::Stdout>>,
    waiters: TextWaiters,
    navigations: NavigateWaiters,
    /// The identity this host owns, for the panel's journals and plans.
    ///
    /// The panel shows what the host reads; the extension opens no database.
    session: Arc<Session>,
    /// Where the panel's feed of the event log stands.
    ///
    /// The panel is one more consumer of the M6-c log and follows its cursor
    /// rules: the position it was last shown is kept here, and a position the
    /// log can no longer replay comes back as `resync_required` rather than
    /// being quietly restarted.
    panel_cursor: Arc<Mutex<i64>>,
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
    let identity_json = session.paths.identity_json();
    let generation = session.identity.generation.to_string();
    let host = Host {
        broker: Arc::new(tokio::sync::Mutex::new(broker)),
        stdout: Arc::new(Mutex::new(io::stdout())),
        waiters: TextWaiters::default(),
        navigations: NavigateWaiters::default(),
        session: Arc::new(session),
        panel_cursor: Arc::new(Mutex::new(0)),
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
        identity_key: host.session.identity.key.to_string(),
        origin: host.session.identity.origin.clone(),
        pause_hidden_after_ms: pause_after,
    });

    let messages = read_stdin();
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
                // The panel's status feed. A submission that finished in
                // another process moves the log, and the panel is shown the
                // new journals. Nothing polls Canvas: this reads the local
                // log the same way `canvas watch` does.
                if log_moved(host).await {
                    push_panel(host).await;
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
            push_panel(host).await;
        }
        ExtensionMessage::NavigateAck {
            request_id,
            accepted,
            reason,
        } => {
            let waiter = host
                .navigations
                .lock()
                .ok()
                .and_then(|mut waiters| waiters.remove(&request_id));
            if let Some(waiter) = waiter {
                let outcome = if accepted {
                    Ok(())
                } else {
                    // The companion named a refusal; anything it does not
                    // name is a stale generation, because the extension is
                    // the side that can see whether the tab moved on.
                    Err(reason
                        .as_deref()
                        .and_then(parse_reason)
                        .unwrap_or(Reason::StaleGeneration))
                };
                let _ = waiter.send(outcome);
            }
        }
        ExtensionMessage::NavigateOutcome {
            request_id,
            outcome,
        } => {
            let load = match outcome.as_str() {
                "loaded" => LoadOutcome::Loaded,
                "failed" => LoadOutcome::Failed,
                _ => LoadOutcome::Unknown,
            };
            let at = crate::output::generated_at_now();
            host.broker.lock().await.navigated(&request_id, load, &at);
            push_panel(host).await;
        }
        ExtensionMessage::Decision {
            plan_id,
            handle,
            plan_sha256,
            decision,
        } => {
            decide(host, &plan_id, &handle, &plan_sha256, &decision).await;
        }
        ExtensionMessage::PanelHello { protocol } => {
            if protocol != NATIVE_PROTOCOL {
                host.send(&HostMessage::Refused {
                    reason: Reason::Protocol.as_str().to_owned(),
                });
                return;
            }
            // A panel that just opened has seen nothing, so its feed starts
            // at the beginning of what the log still holds.
            set_cursor(host, 0);
            push_panel(host).await;
        }
    }
}

/// Parse a refusal the companion named on a navigation acknowledgement.
fn parse_reason(raw: &str) -> Option<Reason> {
    match raw {
        "not_attached" => Some(Reason::NotAttached),
        "paused" => Some(Reason::Paused),
        "origin_mismatch" => Some(Reason::OriginMismatch),
        "stale_generation" => Some(Reason::StaleGeneration),
        _ => None,
    }
}

/// Apply one panel decision, then show the panel what it did.
///
/// Every check runs before anything moves, and a failed check changes
/// nothing. The person is told on stderr which check failed; the panel is
/// told by the state it gets back, which still lists the plan.
async fn decide(host: &Arc<Host>, plan_id: &str, handle: &str, digest: &str, decision: &str) {
    let Some(decision) = Decision::parse(decision) else {
        let _ = writeln!(
            io::stderr(),
            "refusing a panel decision: {}",
            panel::DecisionRefusal::UnknownDecision.as_str()
        );
        push_panel(host).await;
        return;
    };
    let session = Arc::clone(&host.session);
    let plan = plan_id.to_owned();
    let plan_id = plan_id.to_owned();
    let handle = handle.to_owned();
    let digest = digest.to_owned();
    let applied = tokio::task::spawn_blocking(move || {
        panel::apply_decision(&session, &plan_id, &handle, &digest, decision)
    })
    .await;
    match applied {
        Ok(Ok(())) => {
            // The person's own record of what they just did. The plan id is
            // an id; nothing about the work itself is written here.
            let _ = writeln!(io::stderr(), "panel {}: plan {plan}", decision.as_str());
        }
        Ok(Err(refusal)) => {
            let _ = writeln!(
                io::stderr(),
                "refusing a panel decision: {}",
                refusal.as_str()
            );
        }
        Err(_) => {
            let _ = writeln!(io::stderr(), "the panel decision could not be applied");
        }
    }
    push_panel(host).await;
}

/// Compute the panel's whole view and send it.
///
/// The bundle the panel sees is the CLI's own: the host reads it with neither
/// an attachment id nor a consumer, which is the reading REPORT §3.2 permits
/// the local side. It never asks the browser for text.
async fn push_panel(host: &Arc<Host>) {
    let context = host.broker.lock().await.context(None, None, false).ok();
    let session = Arc::clone(&host.session);
    let since = read_cursor(host);
    let state = tokio::task::spawn_blocking(move || {
        Box::new(panel::state(&session, context.as_ref(), since))
    })
    .await;
    let state = state.unwrap_or_else(|_| Box::new(PanelState::default()));
    set_cursor(host, state.cursor);
    host.send(&HostMessage::Panel { state });
}

/// The position the panel was last shown.
fn read_cursor(host: &Arc<Host>) -> i64 {
    match host.panel_cursor.lock() {
        Ok(cursor) => *cursor,
        Err(poisoned) => *poisoned.into_inner(),
    }
}

fn set_cursor(host: &Arc<Host>, mark: i64) {
    match host.panel_cursor.lock() {
        Ok(mut cursor) => *cursor = mark,
        Err(poisoned) => *poisoned.into_inner() = mark,
    }
}

/// Whether the event log has moved past what the panel was last shown.
async fn log_moved(host: &Arc<Host>) -> bool {
    let session = Arc::clone(&host.session);
    let since = read_cursor(host);
    let mark = tokio::task::spawn_blocking(move || panel::cursor(&session, since).0).await;
    mark.is_ok_and(|mark| mark > since)
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
        Op::Note {
            attachment_id,
            consumer,
            generation,
            text,
            source_refs,
        } => {
            let at = crate::output::generated_at_now();
            let stored = host.broker.lock().await.note(
                attachment_id.as_deref(),
                consumer.as_deref(),
                generation,
                &text,
                &source_refs,
                &at,
            );
            match stored {
                Ok(note) => {
                    let (attachment_id, held) = {
                        let broker = host.broker.lock().await;
                        (
                            broker.attachment_id().unwrap_or_default().to_owned(),
                            broker.notes().len() as u64,
                        )
                    };
                    // The panel is told twice on purpose: the note arrives on
                    // its own so a live panel can append one row, and the
                    // whole state follows so a panel that just opened is
                    // consistent.
                    host.send(&HostMessage::Note {
                        note: Box::new(note.clone()),
                    });
                    push_panel(host).await;
                    Body::Noted {
                        note: Box::new(note),
                        attachment_id,
                        held,
                    }
                }
                Err(reason) => Body::Refused { reason },
            }
        }
        Op::Follow {
            attachment_id,
            consumer,
            generation,
            url,
        } => follow(host, attachment_id, consumer, generation, url).await,
    }
}

/// The `follow` operation: dispatch a navigation and answer the
/// acknowledgement alone.
///
/// The capability, the generation, and the granted origin are settled before
/// the browser hears anything, so an unentitled caller and a cross-origin
/// target never move the person's tab. What comes back is the dispatch
/// acknowledgement; whether the page loaded arrives later, as a state on the
/// bundle (REPORT §3.2).
async fn follow(
    host: &Arc<Host>,
    attachment_id: Option<String>,
    consumer: Option<String>,
    generation: u64,
    url: String,
) -> Body {
    if let Err(reason) = host.broker.lock().await.may_follow(
        attachment_id.as_deref(),
        consumer.as_deref(),
        generation,
        &url,
    ) {
        return Body::Refused { reason };
    }
    let request_id = state::new_attachment_id();
    let (tx, rx) = oneshot::channel();
    if let Ok(mut waiters) = host.navigations.lock() {
        waiters.insert(request_id.clone(), tx);
    }
    let started = std::time::Instant::now();
    host.send(&HostMessage::Navigate {
        request_id: request_id.clone(),
        url: url.clone(),
    });
    let acknowledged = match tokio::time::timeout(NAVIGATE_TIMEOUT, rx).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(_)) => Err(Reason::NotAttached),
        Err(_) => {
            if let Ok(mut waiters) = host.navigations.lock() {
                waiters.remove(&request_id);
            }
            Err(Reason::NavigationTimeout)
        }
    };
    if let Err(reason) = acknowledged {
        return Body::Refused { reason };
    }
    let follow = FollowStatus {
        request_id,
        url,
        dispatched: true,
        dispatched_at: crate::output::generated_at_now(),
        dispatch_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        generation,
        // The tab is on its way. Saying anything else here would be a guess.
        load: LoadOutcome::Unknown,
        load_at: None,
    };
    host.broker
        .lock()
        .await
        .dispatched(consumer.as_deref(), follow.clone());
    let attachment_id = host
        .broker
        .lock()
        .await
        .attachment_id()
        .unwrap_or_default()
        .to_owned();
    push_panel(host).await;
    Body::Followed {
        follow: Box::new(follow),
        attachment_id,
    }
}

/// The `here` operation, with the fresh probe a text release requires.
///
/// Metadata is served from the broker's last validated state. Text is
/// released only after the extension re-probed the account for **this**
/// document (REPORT §3.3 step 4), and an account that no longer matches
/// refuses the whole answer rather than returning a bundle without it.
///
/// The order matters: the caller's capability is checked before the probe is
/// asked for, so an unentitled caller costs the browser nothing and learns
/// nothing about the account.
async fn here(
    host: &Arc<Host>,
    attachment_id: Option<String>,
    consumer: Option<String>,
    include_text: bool,
) -> Body {
    // The capability first, and only then the browser. A caller that may not
    // read this attachment must not be able to make the companion re-probe
    // the account or read the page on its behalf.
    if let Err(reason) = host
        .broker
        .lock()
        .await
        .may_read(attachment_id.as_deref(), consumer.as_deref())
    {
        return Body::Refused { reason };
    }
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
