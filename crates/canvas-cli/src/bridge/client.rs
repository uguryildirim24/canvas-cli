//! The client half of `bridge-ipc@1`.
//!
//! `canvas here`, `bridge status`, `bridge detach`, `identity remove`, and the
//! three `context.*` tools all reach the broker through this. It is a
//! blocking Unix-socket client on purpose: every caller is one short request
//! and one short answer, and the command cores this crate runs are `!Send`.

use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

use canvas_core::bridge::Endpoint;
use canvas_core::bridge::ipc::{self, Body, Op, Reason};

/// How long a broker call may take before it is treated as absent.
///
/// It must exceed the host's own wait for the companion's answer
/// (`bridge::host::TEXT_TIMEOUT`), or a slow page would race the two clocks
/// and `canvas here --text` would report `bridge_unavailable` for a broker
/// that was about to answer `validating`.
const TIMEOUT: Duration = Duration::from_secs(15);

/// The longest response line the client will read, in bytes.
///
/// One bundle carries at most the 64 KiB payload plus its metadata; this is
/// the same defence the broker applies to requests, in the other direction.
const MAX_RESPONSE_BYTES: u64 = 256 * 1024;

/// A connection to one identity's broker.
pub struct BridgeClient {
    #[cfg(unix)]
    stream: std::os::unix::net::UnixStream,
}

impl BridgeClient {
    /// Connect to the endpoint, or report that no broker is running.
    ///
    /// An absent broker is [`Reason::BridgeUnavailable`], which the design note
    /// maps to a domain refusal and exit 8 — not a local failure.
    #[cfg(unix)]
    pub fn connect(endpoint: &Endpoint) -> Result<Self, Reason> {
        let stream = std::os::unix::net::UnixStream::connect(&endpoint.socket)
            .map_err(|_| Reason::BridgeUnavailable)?;
        stream
            .set_read_timeout(Some(TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(TIMEOUT)))
            .map_err(|_| Reason::BridgeUnavailable)?;
        Ok(Self { stream })
    }

    /// Windows uses a named pipe, which this package compiled but did not run
    /// (`docs/companion.md`).
    #[cfg(not(unix))]
    pub fn connect(_endpoint: &Endpoint) -> Result<Self, Reason> {
        Err(Reason::BridgeUnavailable)
    }

    /// Send one operation and read its answer.
    #[cfg(unix)]
    pub fn call(&mut self, op: Op) -> Result<Body, Reason> {
        let request = ipc::Request::new(next_id(), op);
        let mut line = serde_json::to_vec(&request).map_err(|_| Reason::Protocol)?;
        line.push(b'\n');
        self.stream
            .write_all(&line)
            .and_then(|()| self.stream.flush())
            .map_err(|_| Reason::BridgeUnavailable)?;
        let mut reader = BufReader::new(&self.stream).take(MAX_RESPONSE_BYTES);
        let mut answer = String::new();
        let read = reader
            .read_line(&mut answer)
            .map_err(|_| Reason::BridgeUnavailable)?;
        if read == 0 {
            return Err(Reason::BridgeUnavailable);
        }
        let response: ipc::Response =
            serde_json::from_str(answer.trim()).map_err(|_| Reason::Protocol)?;
        if response.v != ipc::IPC_PROTOCOL || response.id != request.id {
            return Err(Reason::Protocol);
        }
        match response.body {
            Body::Refused { reason } => Err(reason),
            body => Ok(body),
        }
    }

    #[cfg(not(unix))]
    pub fn call(&mut self, _op: Op) -> Result<Body, Reason> {
        Err(Reason::BridgeUnavailable)
    }
}

#[cfg(unix)]
use std::io::Read as _;

/// A per-call correlation id. It is opaque and never leaves this machine.
#[cfg(unix)]
fn next_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Run one operation against the broker of `endpoint`.
pub fn call(endpoint: &Endpoint, op: Op) -> Result<Body, Reason> {
    BridgeClient::connect(endpoint)?.call(op)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use canvas_core::bridge::ipc::{AttachmentState, Response};
    use canvas_core::identity::IdentityKey;
    use std::os::unix::net::UnixListener;

    fn endpoint(root: &std::path::Path) -> Endpoint {
        Endpoint::for_identity(root, &IdentityKey::compute("https://school.test", 12345))
    }

    /// Serve one request with a fixed answer, on a background thread.
    fn serve(endpoint: &Endpoint, answer: impl Fn(ipc::Request) -> Response + Send + 'static) {
        std::fs::create_dir_all(&endpoint.dir).expect("dir");
        let listener = UnixListener::bind(&endpoint.socket).expect("bind");
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut line = String::new();
            reader.read_line(&mut line).expect("read");
            let request = ipc::parse_request(line.trim()).expect("a well-formed request");
            let mut out = serde_json::to_vec(&answer(request)).expect("encode");
            out.push(b'\n');
            let mut stream = stream;
            stream.write_all(&out).expect("write");
        });
    }

    /// The client must outlast the host's own wait for the companion.
    #[test]
    fn the_client_waits_longer_than_the_host_waits_for_the_page() {
        assert!(
            TIMEOUT > crate::bridge::host::TEXT_TIMEOUT,
            "a slow page would read as an absent broker"
        );
    }

    #[test]
    fn an_absent_broker_is_a_domain_refusal() {
        let dir = tempfile::tempdir().expect("temp");
        assert_eq!(
            call(&endpoint(dir.path()), Op::AttachmentsList).unwrap_err(),
            Reason::BridgeUnavailable
        );
    }

    #[test]
    fn a_call_round_trips_and_matches_its_own_id() {
        let dir = tempfile::tempdir().expect("temp");
        let endpoint = endpoint(dir.path());
        serve(&endpoint, |request| {
            Response::ok(
                request.id,
                Body::Attached {
                    attachment_id: "0123456789abcdef0123456789abcdef".to_owned(),
                    state: AttachmentState::Attached,
                },
            )
        });
        let body = call(
            &endpoint,
            Op::Attach {
                consumer: "cli".to_owned(),
                attachment_id: None,
            },
        )
        .expect("an answer");
        assert!(matches!(body, Body::Attached { .. }));
    }

    #[test]
    fn a_refusal_comes_back_as_its_reason() {
        let dir = tempfile::tempdir().expect("temp");
        let endpoint = endpoint(dir.path());
        serve(&endpoint, |request| {
            Response::refused(request.id, Reason::NotAttached)
        });
        assert_eq!(
            call(&endpoint, Op::AttachmentsList).unwrap_err(),
            Reason::NotAttached
        );
    }

    /// An answer that names another request is not this call's answer.
    #[test]
    fn a_mismatched_correlation_id_is_a_protocol_failure() {
        let dir = tempfile::tempdir().expect("temp");
        let endpoint = endpoint(dir.path());
        serve(&endpoint, |_| {
            Response::ok("another-request", Body::Detached { detached: true })
        });
        assert_eq!(
            call(&endpoint, Op::AttachmentsList).unwrap_err(),
            Reason::Protocol
        );
    }
}
