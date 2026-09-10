//! Admission and owner file locks (§12.2 / §9).

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use std::fs::File;
use std::path::{Path, PathBuf};

use fs4::fs_std::FileExt;

use crate::journal::state::OwnerStatus;

/// Lock-layer errors.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Admission lock held elsewhere.
    #[error("in_progress")]
    InProgress,
    /// I/O failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Exclusive admission lock for one assignment.
#[derive(Debug)]
pub struct AdmissionLock {
    file: File,
    path: PathBuf,
}

/// Exclusive owner lock for one journal.
#[derive(Debug)]
pub struct OwnerLock {
    file: File,
    path: PathBuf,
}

impl Drop for AdmissionLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

impl Drop for OwnerLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn journals_dir(identity_dir: &Path) -> PathBuf {
    identity_dir.join("journals")
}

fn ensure_lock_file(path: &Path) -> Result<File, LockError> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let root = Dir::open_ambient_dir(
        parent
            .parent()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?,
        cap_std::ambient_authority(),
    )?;
    match root.create_dir("journals") {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let dir = root.open_dir_nofollow("journals")?;
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let mut opts = OpenOptions::new();
    opts.read(true)
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = match dir.open_with(name, &opts) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            opts.create_new(false);
            dir.open_with(name, &opts)?
        }
        Err(e) => return Err(e.into()),
    };
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput).into());
    }
    Ok(file.into_std())
}

fn validate_id(id: &str) -> Result<(), LockError> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput).into());
    }
    Ok(())
}

/// Reject a lock name that could name anything but a lock file.
///
/// Admission names are built from ids, so the alphabet is deliberately narrow:
/// letters, digits, and `-`. Nothing here can climb out of `journals/`.
fn validate_lock_name(name: &str) -> Result<(), LockError> {
    if name.is_empty()
        || name.len() > 120
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput).into());
    }
    Ok(())
}

impl AdmissionLock {
    pub(crate) fn matches(&self, identity_dir: &Path, assignment: i64) -> bool {
        self.path == journals_dir(identity_dir).join(format!("assignment-{assignment}.lock"))
    }

    /// Whether this lock is the named admission lock (M8-b).
    #[must_use]
    pub fn matches_named(&self, identity_dir: &Path, name: &str) -> bool {
        self.path == journals_dir(identity_dir).join(format!("{name}.lock"))
    }

    /// Non-blocking exclusive acquire. `Err(InProgress)` if held.
    pub fn try_acquire(identity_dir: &Path, assignment_id: i64) -> Result<Self, LockError> {
        Self::try_acquire_named(identity_dir, &format!("assignment-{assignment_id}"))
    }

    /// Non-blocking exclusive acquire of a named admission lock (M8-b).
    ///
    /// The submission journal admits one operation per assignment, so its lock
    /// is named after the assignment. An operation journal admits one operation
    /// per target, and a target is a topic, a conversation, or one new
    /// conversation, so the name comes from the caller.
    pub fn try_acquire_named(identity_dir: &Path, name: &str) -> Result<Self, LockError> {
        validate_lock_name(name)?;
        let path = journals_dir(identity_dir).join(format!("{name}.lock"));
        let file = ensure_lock_file(&path)?;
        #[cfg(test)]
        super::crash_tests::checkpoint("admission_file_created", "");
        if !FileExt::try_lock_exclusive(&file)? {
            return Err(LockError::InProgress);
        }
        Ok(Self { file, path })
    }
}

impl OwnerLock {
    pub(crate) fn identity_dir(&self) -> &Path {
        self.path
            .parent()
            .and_then(Path::parent)
            .expect("owner path has identity parent")
    }

    pub(crate) fn matches(&self, journal_id: &str) -> bool {
        self.path
            .file_name()
            .is_some_and(|s| s == std::ffi::OsStr::new(&format!("{journal_id}.lock")))
    }

    /// Blocking exclusive acquire (owner path before insert).
    pub fn acquire(identity_dir: &Path, journal_id: &str) -> Result<Self, LockError> {
        validate_id(journal_id)?;
        let path = journals_dir(identity_dir).join(format!("{journal_id}.lock"));
        let file = ensure_lock_file(&path)?;
        #[cfg(test)]
        super::crash_tests::checkpoint("owner_file_created", journal_id);
        FileExt::lock_exclusive(&file)?;
        Ok(Self { file, path })
    }

    /// Non-blocking exclusive acquire. `Ok(None)` if held by a live owner.
    pub fn try_acquire(identity_dir: &Path, journal_id: &str) -> Result<Option<Self>, LockError> {
        validate_id(journal_id)?;
        let path = journals_dir(identity_dir).join(format!("{journal_id}.lock"));
        let file = ensure_lock_file(&path)?;
        if FileExt::try_lock_exclusive(&file)? {
            Ok(Some(Self { file, path }))
        } else {
            Ok(None)
        }
    }
}

/// Non-blocking owner probe: `Live` if lock held, else `Absent` (lock released immediately).
pub fn probe_owner(identity_dir: &Path, journal_id: &str) -> Result<OwnerStatus, LockError> {
    match OwnerLock::try_acquire(identity_dir, journal_id) {
        Ok(Some(_lock)) => Ok(OwnerStatus::Absent),
        Ok(None) => Ok(OwnerStatus::Live),
        Err(e) => Err(e),
    }
}
