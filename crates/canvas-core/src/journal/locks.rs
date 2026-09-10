//! Admission and owner file locks (§12.2 / §9).

use std::fs::{File, OpenOptions};
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
    #[allow(dead_code)]
    path: PathBuf,
}

/// Exclusive owner lock for one journal.
#[derive(Debug)]
pub struct OwnerLock {
    file: File,
    #[allow(dead_code)]
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
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(f) => Ok(f),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?),
        Err(e) => Err(e.into()),
    }
}

impl AdmissionLock {
    /// Non-blocking exclusive acquire. `Err(InProgress)` if held.
    pub fn try_acquire(identity_dir: &Path, assignment_id: i64) -> Result<Self, LockError> {
        let path = journals_dir(identity_dir).join(format!("assignment-{assignment_id}.lock"));
        let file = ensure_lock_file(&path)?;
        if !FileExt::try_lock_exclusive(&file)? {
            return Err(LockError::InProgress);
        }
        Ok(Self { file, path })
    }
}

impl OwnerLock {
    /// Blocking exclusive acquire (owner path before insert).
    pub fn acquire(identity_dir: &Path, journal_id: &str) -> Result<Self, LockError> {
        let path = journals_dir(identity_dir).join(format!("{journal_id}.lock"));
        let file = ensure_lock_file(&path)?;
        FileExt::lock_exclusive(&file)?;
        Ok(Self { file, path })
    }

    /// Non-blocking exclusive acquire. `Ok(None)` if held by a live owner.
    pub fn try_acquire(
        identity_dir: &Path,
        journal_id: &str,
    ) -> Result<Option<Self>, LockError> {
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
#[must_use]
pub fn probe_owner(identity_dir: &Path, journal_id: &str) -> OwnerStatus {
    match OwnerLock::try_acquire(identity_dir, journal_id) {
        Ok(Some(_lock)) => OwnerStatus::Absent,
        Ok(None) => OwnerStatus::Live,
        Err(_) => OwnerStatus::Absent,
    }
}
