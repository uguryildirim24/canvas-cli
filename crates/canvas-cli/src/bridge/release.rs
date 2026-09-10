//! Let a live broker go before `identity remove` takes the exclusive lock.
//!
//! SPEC §10 gives every reader a shared identity lock, and the broker is a
//! long-lived reader. Removing the identity needs the exclusive lock, so the
//! broker is asked to let go first (`bridge-ipc@1` `release`), and the host
//! detaches, tells the extension, and exits.
//!
//! The request is cooperative, never forced. Nothing here kills a process,
//! and nothing here deletes a lock file that some other process may be
//! holding. When the broker does not let go, `identity remove` reports that
//! the identity is busy and changes nothing.

use std::fs;
use std::time::{Duration, Instant};

use canvas_core::bridge::Endpoint;
use canvas_core::bridge::ipc::{Body, Op, Reason};

use super::{client, owner};

/// How long to wait for a released broker to drop its ownership lock.
const RELEASE_TIMEOUT: Duration = Duration::from_secs(5);

/// How often to look again while waiting.
const POLL: Duration = Duration::from_millis(50);

/// What asking the broker to let go produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Released {
    /// No broker held this identity.
    NotRunning,
    /// The broker let go and its ownership lock is free.
    LetGo,
    /// A broker is still holding the identity. Nothing was changed.
    Busy(String),
}

/// Ask the broker of one identity to let go, and wait for it.
pub fn request(endpoint: &Endpoint, reason: &str) -> Released {
    if owner::live_owner(endpoint).is_none() {
        return Released::NotRunning;
    }
    match client::call(
        endpoint,
        Op::Release {
            reason: reason.to_owned(),
        },
    ) {
        Ok(Body::Released { released: true }) => {}
        // The endpoint went away between the two calls: nobody holds it now.
        Err(Reason::BridgeUnavailable) => return wait_for_release(endpoint),
        Ok(_) | Err(_) => {
            return Released::Busy(describe(endpoint));
        }
    }
    wait_for_release(endpoint)
}

/// Wait until the ownership lock is free, or give up and report it.
fn wait_for_release(endpoint: &Endpoint) -> Released {
    let deadline = Instant::now() + RELEASE_TIMEOUT;
    loop {
        if owner::live_owner(endpoint).is_none() {
            return Released::LetGo;
        }
        if Instant::now() >= deadline {
            return Released::Busy(describe(endpoint));
        }
        std::thread::sleep(POLL);
    }
}

/// A line naming what still holds the identity.
fn describe(endpoint: &Endpoint) -> String {
    match owner::live_owner(endpoint) {
        Some(record) => format!(
            "a canvas bridge host (pid {}) still holds this identity",
            record.pid
        ),
        None => "a canvas bridge host still holds this identity".to_owned(),
    }
}

/// Remove one identity's broker endpoint after the identity itself is gone.
///
/// The socket and the ownership lock belong to this identity alone, so both
/// go. The broker directory stays, because other identities live in it, and
/// the root identity lock under `locks/` is not touched here at all.
pub fn forget_endpoint(endpoint: &Endpoint) {
    let _ = fs::remove_file(&endpoint.socket);
    let _ = fs::remove_file(&endpoint.lock);
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas_core::identity::IdentityKey;

    fn endpoint(root: &std::path::Path) -> Endpoint {
        let key = IdentityKey::parse("school.test-7-abcd1234").expect("key");
        let e = Endpoint::for_identity(root, &key);
        owner::ensure_dir(&e.dir).expect("dir");
        e
    }

    /// Nothing to release is not a refusal: the identity is simply free.
    #[test]
    fn an_identity_no_broker_holds_is_not_busy() {
        let dir = tempfile::tempdir().expect("temp");
        assert_eq!(
            request(&endpoint(dir.path()), "identity remove"),
            Released::NotRunning
        );
    }

    /// M7-a acceptance: a live host that will not let go reports busy, and
    /// nothing on disk is changed.
    #[test]
    fn a_live_host_that_does_not_answer_reports_busy() {
        let dir = tempfile::tempdir().expect("temp");
        let endpoint = endpoint(dir.path());
        // Hold the ownership lock the way a running host does, and offer no
        // endpoint at all, so the release request cannot be answered.
        let record = owner::OwnerRecord {
            pid: std::process::id(),
            started_at: "2026-09-10T16:00:00Z".to_owned(),
            identity_key: "school.test-7-abcd1234".to_owned(),
            endpoint: endpoint.address(),
        };
        let _held = owner::Ownership::take(&endpoint, &record).expect("take");
        match request(&endpoint, "identity remove") {
            Released::Busy(message) => assert!(message.contains("still holds"), "{message}"),
            other => panic!("expected busy, got {other:?}"),
        }
        assert!(endpoint.lock.exists(), "the ownership lock was removed");
    }

    /// Removing the endpoint takes this identity's two files and no more.
    #[test]
    fn forgetting_an_endpoint_leaves_the_directory_and_its_neighbours() {
        let dir = tempfile::tempdir().expect("temp");
        let endpoint = endpoint(dir.path());
        fs::write(&endpoint.socket, b"").expect("socket");
        fs::write(&endpoint.lock, b"{}").expect("lock");
        let neighbour = endpoint.dir.join("other.test-9-99999999.lock");
        fs::write(&neighbour, b"{}").expect("neighbour");
        let root_lock = dir.path().join("locks");
        fs::create_dir_all(&root_lock).expect("locks");
        fs::write(root_lock.join("school.test-7-abcd1234.lock"), b"").expect("root lock");

        forget_endpoint(&endpoint);

        assert!(!endpoint.socket.exists());
        assert!(!endpoint.lock.exists());
        assert!(endpoint.dir.is_dir(), "the broker directory was removed");
        assert!(neighbour.exists(), "another identity's lock was removed");
        assert!(
            root_lock.join("school.test-7-abcd1234.lock").exists(),
            "the root identity lock was removed"
        );
    }
}
