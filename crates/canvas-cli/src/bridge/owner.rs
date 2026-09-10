//! Broker ownership: one host per identity, elected by a file lock.
//!
//! `<data root>/bridge/<identity-key>.lock` is a persistent identity-local
//! lock (REPORT §3.6): it is created once and removed only by
//! `identity remove`. Its content is the owner's own description, rewritten
//! each time ownership is taken, so a second host can report who holds it
//! instead of replacing it.
//!
//! The socket is not a lock file. It is transient IPC, so a stale one may be
//! unlinked — but only by a process that already holds this lock, which by
//! construction means no live host owns the endpoint (REPORT §3.4).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use canvas_core::bridge::Endpoint;
use canvas_core::bridge::endpoint::{DIR_MODE, SOCKET_MODE};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};

/// What the lock file says about the process that holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerRecord {
    pub pid: u32,
    pub started_at: String,
    pub identity_key: String,
    /// The endpoint this owner serves.
    pub endpoint: String,
}

/// The ownership lock, held for the host's lifetime.
#[derive(Debug)]
pub struct Ownership {
    /// Holding the open file is holding the lock: dropping it releases.
    _file: File,
    endpoint: Endpoint,
}

/// Why ownership could not be taken.
#[derive(Debug, thiserror::Error)]
pub enum OwnerError {
    /// Another host owns this identity. It is reported, never replaced.
    #[error("another canvas bridge host already owns this identity")]
    Taken(Box<Option<OwnerRecord>>),
    #[error("{0}")]
    Io(#[from] io::Error),
}

impl Ownership {
    /// Create the broker directory and take the lock, or report the owner.
    pub fn take(endpoint: &Endpoint, record: &OwnerRecord) -> Result<Self, OwnerError> {
        ensure_dir(&endpoint.dir)?;
        let mut file = open_lock(&endpoint.lock)?;
        if !FileExt::try_lock_exclusive(&file)? {
            return Err(OwnerError::Taken(Box::new(read_record(&endpoint.lock))));
        }
        // The lock is ours: rewrite the description of who holds it.
        file.seek(SeekFrom::Start(0))?;
        file.set_len(0)?;
        file.write_all(&serde_json::to_vec_pretty(record).unwrap_or_default())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(Self {
            _file: file,
            endpoint: endpoint.clone(),
        })
    }

    /// Remove a socket file left behind by an absent owner.
    ///
    /// Safe only because this process holds the ownership lock: no live host
    /// can be serving the endpoint while that is true.
    pub fn clear_stale_socket(&self) -> io::Result<bool> {
        match fs::symlink_metadata(&self.endpoint.socket) {
            Ok(_) => {
                fs::remove_file(&self.endpoint.socket)?;
                Ok(true)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Tighten the endpoint to mode `0600` once it exists.
    pub fn restrict_socket(&self) -> io::Result<()> {
        set_mode(&self.endpoint.socket, SOCKET_MODE)
    }
}

/// Whether a live host owns this identity, and what it says about itself.
///
/// A shared lock that can be taken means no exclusive holder exists, so the
/// owner is absent. The probe releases immediately and changes nothing.
#[must_use]
pub fn live_owner(endpoint: &Endpoint) -> Option<OwnerRecord> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&endpoint.lock)
        .ok()?;
    match FileExt::try_lock_shared(&file) {
        Ok(true) => {
            let _ = FileExt::unlock(&file);
            None
        }
        // Someone holds it exclusively: a host is live.
        Ok(false) => read_record(&endpoint.lock),
        Err(_) => None,
    }
}

/// Create the broker directory at mode `0700`.
pub fn ensure_dir(dir: &Path) -> io::Result<()> {
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }
    set_mode(dir, DIR_MODE)
}

/// Read the owner description, if the file holds one.
fn read_record(path: &Path) -> Option<OwnerRecord> {
    let mut raw = String::new();
    File::open(path).ok()?.read_to_string(&mut raw).ok()?;
    serde_json::from_str(raw.trim()).ok()
}

/// Open the lock file, creating it if it is not there yet.
fn open_lock(path: &Path) -> io::Result<File> {
    match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => {
            set_mode(path, SOCKET_MODE)?;
            Ok(file)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            OpenOptions::new().read(true).write(true).open(path)
        }
        Err(e) => Err(e),
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    // Windows uses the pipe ACL instead; see `canvas_core::bridge::endpoint`.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas_core::identity::IdentityKey;

    fn record(key: &str, endpoint: &Endpoint) -> OwnerRecord {
        OwnerRecord {
            pid: std::process::id(),
            started_at: "2026-09-10T10:00:00Z".to_owned(),
            identity_key: key.to_owned(),
            endpoint: endpoint.address(),
        }
    }

    fn endpoint(root: &Path) -> (Endpoint, IdentityKey) {
        let key = IdentityKey::compute("https://school.test", 12345);
        (Endpoint::for_identity(root, &key), key)
    }

    #[test]
    fn taking_ownership_creates_a_private_directory_and_names_the_owner() {
        let dir = tempfile::tempdir().expect("temp");
        let (endpoint, key) = endpoint(dir.path());
        let owned = Ownership::take(&endpoint, &record(key.as_str(), &endpoint)).expect("take");
        assert!(endpoint.dir.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&endpoint.dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, DIR_MODE, "the broker directory must be 0700");
        }
        let described = read_record(&endpoint.lock).expect("the owner is described");
        assert_eq!(described.identity_key, key.as_str());
        assert_eq!(described.pid, std::process::id());
        drop(owned);
    }

    /// M7-a acceptance: a second host reports the owner and never replaces it.
    #[test]
    fn a_second_owner_is_refused_and_the_first_is_reported() {
        let dir = tempfile::tempdir().expect("temp");
        let (endpoint, key) = endpoint(dir.path());
        let first = Ownership::take(&endpoint, &record(key.as_str(), &endpoint)).expect("take");
        let mut second_record = record(key.as_str(), &endpoint);
        second_record.pid = std::process::id() + 1;
        match Ownership::take(&endpoint, &second_record) {
            Err(OwnerError::Taken(existing)) => {
                let existing = existing.expect("the first owner described itself");
                assert_eq!(existing.pid, std::process::id());
            }
            other => panic!("a second owner must be refused, got {other:?}"),
        }
        // The first owner's description survives the attempt.
        assert_eq!(read_record(&endpoint.lock).unwrap().pid, std::process::id());
        assert!(live_owner(&endpoint).is_some(), "the owner reads as live");
        drop(first);
    }

    /// M7-a acceptance: a stale socket is cleaned only under ownership, and a
    /// live endpoint is never unlinked.
    #[test]
    fn a_stale_socket_is_cleared_only_by_the_owner() {
        let dir = tempfile::tempdir().expect("temp");
        let (endpoint, key) = endpoint(dir.path());
        ensure_dir(&endpoint.dir).expect("dir");
        fs::write(&endpoint.socket, b"").expect("a leftover endpoint");
        // Without the lock there is no `Ownership`, so there is no method that
        // can unlink it: taking the lock is the only way to reach one.
        let owned = Ownership::take(&endpoint, &record(key.as_str(), &endpoint)).expect("take");
        assert!(owned.clear_stale_socket().expect("clear"));
        assert!(!endpoint.socket.exists());
        assert!(!owned.clear_stale_socket().expect("nothing left"));
        // The lock file itself is never removed.
        assert!(endpoint.lock.exists());
    }

    #[test]
    fn an_absent_owner_reads_as_absent() {
        let dir = tempfile::tempdir().expect("temp");
        let (endpoint, key) = endpoint(dir.path());
        assert!(!endpoint.lock.exists());
        assert_eq!(live_owner(&endpoint), None);
        let owned = Ownership::take(&endpoint, &record(key.as_str(), &endpoint)).expect("take");
        drop(owned);
        assert!(endpoint.lock.exists(), "the lock file is never deleted");
        assert_eq!(live_owner(&endpoint), None, "a released lock is not live");
    }
}
