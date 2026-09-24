//! Where the broker lives on disk (the design note, the §9 path additions).
//!
//! | Purpose | Path |
//! |---|---|
//! | Broker directory (mode `0700`) | `<data root>/bridge/` |
//! | Ownership lock (never deleted) | `<data root>/bridge/<identity-key>.lock` |
//! | Endpoint (mode `0600`) | `<data root>/bridge/<identity-key>.sock` |
//! | Windows endpoint | `\\.\pipe\canvas-cli-<identity-key>` |
//!
//! The lock file is a persistent identity-local lock: it is created once and
//! removed only by `identity remove`. The socket is transient IPC, so a stale
//! one may be unlinked — but only by a process that already holds the
//! ownership lock, and never while it is live.

use std::path::{Path, PathBuf};

use crate::identity::IdentityKey;

/// The broker directory and the two paths inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// `<data root>/bridge/`.
    pub dir: PathBuf,
    /// `<data root>/bridge/<identity-key>.lock`.
    pub lock: PathBuf,
    /// `<data root>/bridge/<identity-key>.sock`.
    pub socket: PathBuf,
    /// `\\.\pipe\canvas-cli-<identity-key>`.
    pub pipe: String,
}

/// The directory name under the data root.
pub const DIR_NAME: &str = "bridge";

/// The mode of the broker directory on Unix.
pub const DIR_MODE: u32 = 0o700;

/// The mode of the endpoint on Unix.
pub const SOCKET_MODE: u32 = 0o600;

/// The longest Unix socket path this code will try to bind.
///
/// `sockaddr_un.sun_path` holds 104 bytes on macOS and 108 on Linux, and the
/// terminating NUL is one of them. The smaller of the two, less one, is the
/// bound that holds everywhere, and a path over it is a plain local failure
/// with an obvious fix rather than an opaque `bind` error.
pub const MAX_SOCKET_PATH: usize = 103;

impl Endpoint {
    /// Build the endpoint paths for one identity under `data_root`.
    #[must_use]
    pub fn for_identity(data_root: impl AsRef<Path>, key: &IdentityKey) -> Self {
        let dir = data_root.as_ref().join(DIR_NAME);
        Self {
            lock: dir.join(format!("{}.lock", key.as_str())),
            socket: dir.join(format!("{}.sock", key.as_str())),
            pipe: pipe_name(key),
            dir,
        }
    }

    /// The endpoint a client connects to on this platform.
    #[must_use]
    pub fn address(&self) -> String {
        if cfg!(windows) {
            self.pipe.clone()
        } else {
            self.socket.display().to_string()
        }
    }

    /// Whether the endpoint path fits in a `sockaddr_un`.
    ///
    /// Windows named pipes carry no such bound, so this is a Unix question.
    #[must_use]
    pub fn path_fits(&self) -> bool {
        cfg!(windows) || self.socket.as_os_str().len() <= MAX_SOCKET_PATH
    }
}

/// The Windows named pipe of one identity.
///
/// Compiled and tested on every platform; the pipe itself is created only on
/// Windows, which this package did not run (`docs/companion.md`).
#[must_use]
pub fn pipe_name(key: &IdentityKey) -> String {
    format!(r"\\.\pipe\canvas-cli-{}", key.as_str())
}

/// A user-restricted SDDL security descriptor for the Windows pipe.
///
/// `D:P` is a protected DACL, so nothing is inherited. The two entries grant
/// all access (`GA`) to the pipe's owner and to `SY` (local system), which
/// cannot be excluded from a named pipe in practice. No entry names
/// `Everyone`, `Authenticated Users`, `Network`, or `Anonymous`, and the
/// descriptor denies network access explicitly by naming no network principal.
///
/// `owner_sid` is the current user's SID at run time; taking it as an argument
/// keeps this function pure and testable on a Unix machine.
#[must_use]
pub fn pipe_sddl(owner_sid: &str) -> String {
    format!("D:P(A;;GA;;;{owner_sid})(A;;GA;;;SY)")
}

/// Whether a string is a well-formed SID this builder will accept.
///
/// `S-1-` followed by a revision and at least one sub-authority, all decimal.
#[must_use]
pub fn is_sid(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("S-1-") else {
        return false;
    };
    let mut parts = rest.split('-');
    let Some(authority) = parts.next() else {
        return false;
    };
    if authority.is_empty() || !authority.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let mut subs = 0;
    for part in parts {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        subs += 1;
    }
    subs >= 1
}

/// Build the pipe security descriptor, refusing a SID that is not one.
pub fn pipe_security(owner_sid: &str) -> Result<String, &'static str> {
    if !is_sid(owner_sid) {
        return Err("the pipe owner must be a SID");
    }
    Ok(pipe_sddl(owner_sid))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> IdentityKey {
        IdentityKey::compute("https://school.test", 12345)
    }

    #[test]
    fn every_path_sits_under_the_broker_directory() {
        let endpoint = Endpoint::for_identity("/data", &key());
        assert_eq!(endpoint.dir, Path::new("/data/bridge"));
        assert_eq!(endpoint.lock.parent(), Some(endpoint.dir.as_path()));
        assert_eq!(endpoint.socket.parent(), Some(endpoint.dir.as_path()));
        assert!(
            endpoint
                .socket
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".sock")
        );
    }

    /// The key is filesystem-safe by construction (§8), so the endpoint name
    /// can never escape its directory.
    #[test]
    fn an_endpoint_name_cannot_traverse() {
        let endpoint = Endpoint::for_identity("/data", &key());
        for path in [&endpoint.lock, &endpoint.socket] {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            assert!(!name.contains('/'), "{name}");
            assert!(!name.contains(".."), "{name}");
        }
    }

    #[test]
    fn the_pipe_name_is_the_documented_one() {
        assert_eq!(
            pipe_name(&key()),
            format!(r"\\.\pipe\canvas-cli-{}", key().as_str())
        );
    }

    #[test]
    fn the_pipe_descriptor_is_protected_and_names_no_open_principal() {
        let sddl = pipe_security("S-1-5-21-1004336348-1177238915-682003330-512").expect("a sid");
        assert!(sddl.starts_with("D:P("), "{sddl}");
        assert!(sddl.contains("S-1-5-21-1004336348-1177238915-682003330-512"));
        // `WD` is Everyone, `AU` Authenticated Users, `AN` Anonymous, `NU`
        // Network. None of them may appear.
        for open in [";WD)", ";AU)", ";AN)", ";NU)", ";IU)"] {
            assert!(!sddl.contains(open), "{sddl} grants {open}");
        }
    }

    #[test]
    fn the_descriptor_builder_refuses_anything_that_is_not_a_sid() {
        for bad in [
            "",
            "not-a-sid",
            "S-1-",
            "S-1-5-",
            "S-1-x-5",
            "D:P(A;;GA;;;WD)",
        ] {
            assert!(pipe_security(bad).is_err(), "{bad}");
        }
        assert!(is_sid("S-1-5-18"));
        assert!(is_sid("S-1-5-21-1-2-3-1001"));
    }
}
