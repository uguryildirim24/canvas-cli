//! Install critical section, clobber table, move protocol, transfer trait.

use std::io::{Read, Seek, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use cap_std::fs::Dir;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::download::contain::{
    ContainError, create_part_file, install_rename, open_contained_file, walk_parent,
};
use crate::download::manifest::{Destination, ManifestError, ManifestRow};

/// Per-file action matching Appendix D `download@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Dry-run / planned path.
    Planned,
    /// Freshly installed or replaced.
    Downloaded,
    /// Renamed via move protocol.
    Moved,
    /// Local match and remote unchanged.
    Skipped,
    /// Path present without our row.
    Unmanaged,
    /// Owned path whose local bytes differ.
    Modified,
    /// Locked for the user.
    Locked,
    /// Listing/API unavailable.
    Unavailable,
    /// External-tool item.
    SkippedExternal,
    /// Symlink / escape refused.
    UnsafePath,
    /// Incomplete move that could not be resolved.
    UnresolvedMove,
    /// Transfer or lock failure.
    Failed,
}

impl Action {
    /// Stable string for JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Downloaded => "downloaded",
            Self::Moved => "moved",
            Self::Skipped => "skipped",
            Self::Unmanaged => "unmanaged",
            Self::Modified => "modified",
            Self::Locked => "locked",
            Self::Unavailable => "unavailable",
            Self::SkippedExternal => "skipped_external",
            Self::UnsafePath => "unsafe_path",
            Self::UnresolvedMove => "unresolved_move",
            Self::Failed => "failed",
        }
    }
}

/// True when this action forces run outcome `partial` (exit 12).
#[must_use]
pub fn makes_partial(action: Action) -> bool {
    matches!(
        action,
        Action::Failed
            | Action::Unavailable
            | Action::UnsafePath
            | Action::UnresolvedMove
            | Action::Locked
    )
}

/// True when any action in the slice forces `partial`.
#[must_use]
pub fn outcome_is_partial(actions: &[Action]) -> bool {
    actions.iter().copied().any(makes_partial)
}

/// Transport error.
#[derive(Debug, Error)]
pub enum TransferError {
    /// Size mismatch or incomplete body.
    #[error("transfer size mismatch")]
    SizeMismatch,
    /// Generic failure.
    #[error("{0}")]
    Message(String),
    /// I/O.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Network transport for file bytes (no HTTP in this crate).
pub trait Transfer: Send + Sync {
    /// Fetch `file_id` into `sink`, returning bytes written. Must match `expected_size`.
    fn fetch(
        &self,
        file_id: i64,
        sink: &mut (dyn Write + Send),
        expected_size: u64,
    ) -> impl std::future::Future<Output = Result<u64, TransferError>> + Send;
}

/// In-memory fake transport for tests.
#[derive(Debug, Clone)]
pub struct FakeTransfer {
    /// `file_id` → bytes.
    pub files: std::collections::HashMap<i64, Vec<u8>>,
}

impl Transfer for FakeTransfer {
    async fn fetch(
        &self,
        file_id: i64,
        sink: &mut (dyn Write + Send),
        expected_size: u64,
    ) -> Result<u64, TransferError> {
        let bytes = self
            .files
            .get(&file_id)
            .ok_or_else(|| TransferError::Message(format!("missing file {file_id}")))?;
        if bytes.len() as u64 != expected_size {
            return Err(TransferError::SizeMismatch);
        }
        sink.write_all(bytes)?;
        Ok(bytes.len() as u64)
    }
}

/// Install / classify errors.
#[derive(Debug, Error)]
pub enum InstallError {
    /// Containment refused.
    #[error(transparent)]
    Contain(#[from] ContainError),
    /// Manifest error.
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    /// Transfer error.
    #[error(transparent)]
    Transfer(#[from] TransferError),
    /// I/O.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Remote metadata used by the clobber table.
#[derive(Debug, Clone)]
pub struct RemoteMeta {
    /// Remote size.
    pub size: u64,
    /// Remote `updated_at`.
    pub updated_at: Option<String>,
}

/// Inputs for clobber classification.
#[derive(Debug, Clone)]
pub struct ClassifyInput {
    /// Final path relative to dest.
    pub path: String,
    /// Canvas file id.
    pub file_id: i64,
    /// Manifest row for this `file_id` (any path).
    pub row_for_file: Option<ManifestRow>,
    /// Whether the final path exists as a regular file.
    pub path_present: bool,
    /// Local size when present.
    pub local_size: Option<u64>,
    /// Local sha256 when `--verify` or needed.
    pub local_sha256: Option<String>,
    /// Remote metadata.
    pub remote: RemoteMeta,
    /// `--force`.
    pub force: bool,
    /// `--verify` (hash compare for local match).
    pub verify: bool,
}

/// Clobber decision before transfer/install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClobberDecision {
    /// Install (new or replace our own).
    Install,
    /// Skip with action.
    Skip(Action),
}

/// Classify per the §12.3 clobber table.
#[must_use]
pub fn classify_clobber(input: &ClassifyInput) -> ClobberDecision {
    if !input.path_present {
        return ClobberDecision::Install;
    }

    let row_at_path = input
        .row_for_file
        .as_ref()
        .filter(|&r| r.path == input.path);

    // present + no row for this file_id at this path
    // Spec: "Manifest row for file_id at this path"
    if row_at_path.is_none() {
        if input.force {
            return ClobberDecision::Install;
        }
        return ClobberDecision::Skip(Action::Unmanaged);
    }

    let row = row_at_path.unwrap();
    let local_matches = local_matches_row(row, input);
    let remote_unchanged = remote_unchanged(row, &input.remote);

    if local_matches && remote_unchanged {
        return ClobberDecision::Skip(Action::Skipped);
    }
    if local_matches && !remote_unchanged {
        return ClobberDecision::Install;
    }
    // local does not match
    if input.force {
        ClobberDecision::Install
    } else {
        ClobberDecision::Skip(Action::Modified)
    }
}

fn local_matches_row(row: &ManifestRow, input: &ClassifyInput) -> bool {
    let Some(local_size) = input.local_size else {
        return false;
    };
    if local_size != row.size {
        return false;
    }
    if input.verify {
        match (&row.sha256, &input.local_sha256) {
            (Some(expected), Some(actual)) => expected == actual,
            _ => false,
        }
    } else {
        true
    }
}

fn remote_unchanged(row: &ManifestRow, remote: &RemoteMeta) -> bool {
    if row.size != remote.size {
        return false;
    }
    match (&row.remote_updated_at, &remote.updated_at) {
        (Some(a), Some(b)) => a == b,
        (None, None) => true,
        _ => false,
    }
}

/// Options for install.
#[derive(Debug, Clone)]
pub struct InstallOpts {
    /// `--force`.
    pub force: bool,
    /// `--verify`.
    pub verify: bool,
    /// Course id for the manifest row.
    pub course_id: i64,
}

/// Transfer into a part file then install under the mutex.
pub async fn install_part_file<T: Transfer>(
    dest: &Destination,
    transfer: &T,
    file_id: i64,
    rel_path: &str,
    remote: &RemoteMeta,
    opts: &InstallOpts,
) -> Result<Action, InstallError> {
    // Classify under lock first (may skip without transfer).
    {
        let _lock_guard = dest
            .acquire_install_lock(crate::download::manifest::LOCK_TIMEOUT)
            .await?;
        let decision = classify_at(dest, file_id, rel_path, remote, opts)?;
        if let ClobberDecision::Skip(action) = decision {
            return Ok(action);
        }
    }

    // Transfer outside the mutex into a part file.
    let contained = walk_parent(&dest.root, rel_path)?;
    let random = random_token();
    let (mut part, part_name) = create_part_file(&contained.parent, &contained.name, &random)?;
    let mut hasher = Sha256::new();
    let mut counting = HashingWriter {
        inner: &mut part,
        hasher: &mut hasher,
        written: 0,
    };
    let written = transfer.fetch(file_id, &mut counting, remote.size).await?;
    if written != remote.size {
        let _ = contained.parent.remove_file(&part_name);
        return Err(TransferError::SizeMismatch.into());
    }
    part.sync_all()?;
    let sha = hex_sha(hasher.finalize().as_slice());

    // Critical section: re-classify, rename, commit.
    let _lock_guard = dest
        .acquire_install_lock(crate::download::manifest::LOCK_TIMEOUT)
        .await?;
    let decision = classify_at(dest, file_id, rel_path, remote, opts)?;
    if let ClobberDecision::Skip(action) = decision {
        let _ = contained.parent.remove_file(&part_name);
        return Ok(action);
    }
    install_rename(&contained.parent, &part_name, &contained.name)?;
    let row = ManifestRow {
        file_id,
        course_id: opts.course_id,
        path: rel_path.to_string(),
        size: remote.size,
        sha256: Some(sha),
        remote_updated_at: remote.updated_at.clone(),
        installed_at: now_rfc3339(),
        pending_move_to: None,
        move_sha256: None,
    };
    dest.manifest.upsert(&row)?;
    Ok(Action::Downloaded)
}

fn classify_at(
    dest: &Destination,
    file_id: i64,
    rel_path: &str,
    remote: &RemoteMeta,
    opts: &InstallOpts,
) -> Result<ClobberDecision, InstallError> {
    let row_for_file = dest.manifest.get(file_id)?;
    let (path_present, local_size, local_sha256) =
        inspect_local(&dest.root, rel_path, opts.verify)?;
    Ok(classify_clobber(&ClassifyInput {
        path: rel_path.to_string(),
        file_id,
        row_for_file,
        path_present,
        local_size,
        local_sha256,
        remote: remote.clone(),
        force: opts.force,
        verify: opts.verify,
    }))
}

fn inspect_local(
    root: &Dir,
    rel_path: &str,
    want_hash: bool,
) -> Result<(bool, Option<u64>, Option<String>), InstallError> {
    let contained = match walk_parent(root, rel_path) {
        Ok(c) => c,
        Err(ContainError::UnsafePath) => return Err(ContainError::UnsafePath.into()),
        Err(e) => return Err(e.into()),
    };
    match contained.parent.symlink_metadata(&contained.name) {
        Ok(meta) if meta.file_type().is_symlink() => Err(ContainError::UnsafePath.into()),
        Ok(meta) if meta.is_file() => {
            let file = open_contained_file(&contained, false)?;
            let size = file.metadata()?.len();
            let sha = if want_hash {
                Some(hash_file_handle(&file)?)
            } else {
                None
            };
            Ok((true, Some(size), sha))
        }
        Ok(_) => Ok((true, None, None)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((false, None, None)),
        Err(e) => Err(e.into()),
    }
}

fn hash_file_handle(file: &cap_std::fs::File) -> Result<String, InstallError> {
    let mut f = file.try_clone()?;
    // Seek to start.
    f.rewind()?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_sha(hasher.finalize().as_slice()))
}

/// Hash a path under root (no-follow). Returns `None` if absent.
pub fn hash_path(root: &Dir, rel_path: &str) -> Result<Option<String>, InstallError> {
    let (present, _, sha) = inspect_local(root, rel_path, true)?;
    if present { Ok(sha) } else { Ok(None) }
}

/// Move-protocol resolution when planned path differs from row path.
pub fn resolve_move(
    dest: &Destination,
    file_id: i64,
    new_path: &str,
    remote: &RemoteMeta,
    force: bool,
) -> Result<MoveOutcome, InstallError> {
    let Some(row) = dest.manifest.get(file_id)? else {
        return Ok(MoveOutcome::DownloadAnew);
    };
    if row.path == new_path {
        return Ok(MoveOutcome::AlreadyThere);
    }

    let old_hash = hash_path(&dest.root, &row.path)?;
    let hash_ok = match (&row.sha256, old_hash) {
        (Some(expected), Some(actual)) => expected == &actual,
        _ => false,
    };
    let remote_ok = remote_unchanged(&row, remote);

    let (new_present, _, _) = inspect_local(&dest.root, new_path, false)?;
    let new_ok = !new_present || force;

    if hash_ok && remote_ok && new_ok {
        let sha = row.sha256.clone().unwrap_or_default();
        dest.manifest.set_pending_move(file_id, new_path, &sha)?;
        // Rename old → new through containment.
        rename_rel(&dest.root, &row.path, new_path)?;
        dest.manifest.finalize_move(file_id, new_path)?;
        return Ok(MoveOutcome::Moved);
    }
    if !remote_ok {
        // Download new revision to new path; old becomes unmanaged.
        return Ok(MoveOutcome::DownloadAnew);
    }
    Ok(MoveOutcome::DownloadAnew)
}

/// Result of attempting the move protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveOutcome {
    /// Paths already equal.
    AlreadyThere,
    /// Move completed.
    Moved,
    /// Caller should download to the new path.
    DownloadAnew,
}

fn rename_rel(root: &Dir, from: &str, to: &str) -> Result<(), InstallError> {
    let from_c = walk_parent(root, from)?;
    let to_c = walk_parent(root, to)?;
    // Ensure target parent exists (walk_parent creates dirs).
    from_c
        .parent
        .rename(&from_c.name, &to_c.parent, &to_c.name)?;
    Ok(())
}

/// Recover `pending_move_to` markers at run start.
pub fn recover_pending_moves(dest: &Destination) -> Result<Vec<(i64, Action)>, InstallError> {
    let mut out = Vec::new();
    for row in dest.manifest.pending_moves()? {
        let Some(new_path) = row.pending_move_to.clone() else {
            continue;
        };
        let Some(expected) = row.move_sha256.clone() else {
            dest.manifest.clear_pending_move(row.file_id)?;
            out.push((row.file_id, Action::UnresolvedMove));
            continue;
        };
        let old_hash = hash_path(&dest.root, &row.path)?;
        let new_hash = hash_path(&dest.root, &new_path)?;
        let old_match = old_hash.as_ref() == Some(&expected);
        let new_match = new_hash.as_ref() == Some(&expected);
        match (old_match, new_match) {
            (false, true) => {
                dest.manifest.finalize_move(row.file_id, &new_path)?;
                out.push((row.file_id, Action::Moved));
            }
            (true, false) => {
                dest.manifest.clear_pending_move(row.file_id)?;
                out.push((row.file_id, Action::Skipped));
            }
            (true, true) => {
                dest.manifest.finalize_move(row.file_id, &new_path)?;
                // Old path still exists with same bytes → unmanaged leftover.
                out.push((row.file_id, Action::Moved));
            }
            (false, false) => {
                dest.manifest.clear_pending_move(row.file_id)?;
                out.push((row.file_id, Action::UnresolvedMove));
            }
        }
    }
    Ok(out)
}

struct HashingWriter<'a, W: Write> {
    inner: &'a mut W,
    hasher: &'a mut Sha256,
    written: u64,
}

impl<W: Write> Write for HashingWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.written += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn hex_sha(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn random_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{nanos:x}")
}

fn now_rfc3339() -> String {
    jiff::Timestamp::now().to_string()
}

#[cfg(test)]
#[allow(clippy::used_underscore_binding, unused_variables)]
mod tests {
    use super::*;
    use crate::download::manifest::{ManifestRow, open_destination};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch() -> std::path::PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("canvas-core-install-{nanos}-{n}"));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn row(path: &str, size: u64, sha: &str, updated: &str) -> ManifestRow {
        ManifestRow {
            file_id: 1,
            course_id: 9,
            path: path.into(),
            size,
            sha256: Some(sha.into()),
            remote_updated_at: Some(updated.into()),
            installed_at: "2026-01-01T00:00:00Z".into(),
            pending_move_to: None,
            move_sha256: None,
        }
    }

    #[test]
    fn clobber_table_rows() {
        let remote = RemoteMeta {
            size: 10,
            updated_at: Some("t1".into()),
        };
        // absent → install
        assert_eq!(
            classify_clobber(&ClassifyInput {
                path: "a".into(),
                file_id: 1,
                row_for_file: None,
                path_present: false,
                local_size: None,
                local_sha256: None,
                remote: remote.clone(),
                force: false,
                verify: false,
            }),
            ClobberDecision::Install
        );
        // present, no row → unmanaged
        assert_eq!(
            classify_clobber(&ClassifyInput {
                path: "a".into(),
                file_id: 1,
                row_for_file: None,
                path_present: true,
                local_size: Some(10),
                local_sha256: None,
                remote: remote.clone(),
                force: false,
                verify: false,
            }),
            ClobberDecision::Skip(Action::Unmanaged)
        );
        // present, no row, force → install
        assert_eq!(
            classify_clobber(&ClassifyInput {
                path: "a".into(),
                file_id: 1,
                row_for_file: None,
                path_present: true,
                local_size: Some(10),
                local_sha256: None,
                remote: remote.clone(),
                force: true,
                verify: false,
            }),
            ClobberDecision::Install
        );
        let r = row("a", 10, "abc", "t1");
        // match + remote unchanged → skipped
        assert_eq!(
            classify_clobber(&ClassifyInput {
                path: "a".into(),
                file_id: 1,
                row_for_file: Some(r.clone()),
                path_present: true,
                local_size: Some(10),
                local_sha256: Some("abc".into()),
                remote: remote.clone(),
                force: false,
                verify: true,
            }),
            ClobberDecision::Skip(Action::Skipped)
        );
        // match + remote changed → install
        let remote2 = RemoteMeta {
            size: 10,
            updated_at: Some("t2".into()),
        };
        assert_eq!(
            classify_clobber(&ClassifyInput {
                path: "a".into(),
                file_id: 1,
                row_for_file: Some(r.clone()),
                path_present: true,
                local_size: Some(10),
                local_sha256: Some("abc".into()),
                remote: remote2,
                force: false,
                verify: true,
            }),
            ClobberDecision::Install
        );
        // modified
        assert_eq!(
            classify_clobber(&ClassifyInput {
                path: "a".into(),
                file_id: 1,
                row_for_file: Some(r),
                path_present: true,
                local_size: Some(11),
                local_sha256: None,
                remote,
                force: false,
                verify: false,
            }),
            ClobberDecision::Skip(Action::Modified)
        );
    }

    #[test]
    fn partial_outcome_mapping() {
        assert!(makes_partial(Action::Failed));
        assert!(makes_partial(Action::Unavailable));
        assert!(makes_partial(Action::UnsafePath));
        assert!(makes_partial(Action::UnresolvedMove));
        assert!(makes_partial(Action::Locked));
        assert!(!makes_partial(Action::Unmanaged));
        assert!(!makes_partial(Action::Modified));
        assert!(!makes_partial(Action::Downloaded));
        assert!(outcome_is_partial(&[Action::Skipped, Action::Locked]));
    }

    #[tokio::test]
    async fn first_run_unmanaged_and_download() {
        let path = scratch();
        let dest = open_destination(&path, "id-1").unwrap();
        std::fs::create_dir_all(path.join("c")).unwrap();
        std::fs::write(path.join("c/preexisting.txt"), b"hello").unwrap();

        let remote = RemoteMeta {
            size: 5,
            updated_at: Some("t".into()),
        };
        let action = install_part_file(
            &dest,
            &FakeTransfer {
                files: std::collections::HashMap::from([(1, b"hello".to_vec())]),
            },
            1,
            "c/preexisting.txt",
            &remote,
            &InstallOpts {
                force: false,
                verify: false,
                course_id: 1,
            },
        )
        .await
        .unwrap();
        assert_eq!(action, Action::Unmanaged);

        let action = install_part_file(
            &dest,
            &FakeTransfer {
                files: std::collections::HashMap::from([(2, b"world".to_vec())]),
            },
            2,
            "c/new.txt",
            &RemoteMeta {
                size: 5,
                updated_at: Some("t".into()),
            },
            &InstallOpts {
                force: false,
                verify: false,
                course_id: 1,
            },
        )
        .await
        .unwrap();
        assert_eq!(action, Action::Downloaded);
        assert_eq!(std::fs::read(path.join("c/new.txt")).unwrap(), b"world");
    }

    #[tokio::test]
    async fn two_competing_installers() {
        let path = scratch();
        let dest = open_destination(&path, "id-1").unwrap();
        let lock_path = path.join(".canvas-cli/install.lock");

        let holder = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        assert!(fs4::fs_std::FileExt::try_lock_exclusive(&holder).unwrap());

        let err = dest
            .acquire_install_lock(std::time::Duration::from_millis(200))
            .await
            .unwrap_err();
        assert!(matches!(err, ManifestError::LockTimeout));
        drop(holder);
    }

    #[tokio::test]
    async fn modified_owned_file() {
        let path = scratch();
        let dest = open_destination(&path, "id-1").unwrap();
        let remote = RemoteMeta {
            size: 4,
            updated_at: Some("t1".into()),
        };
        install_part_file(
            &dest,
            &FakeTransfer {
                files: std::collections::HashMap::from([(1, b"abcd".to_vec())]),
            },
            1,
            "f.txt",
            &remote,
            &InstallOpts {
                force: false,
                verify: false,
                course_id: 1,
            },
        )
        .await
        .unwrap();
        std::fs::write(path.join("f.txt"), b"XX").unwrap();
        let action = install_part_file(
            &dest,
            &FakeTransfer {
                files: std::collections::HashMap::from([(1, b"abcd".to_vec())]),
            },
            1,
            "f.txt",
            &remote,
            &InstallOpts {
                force: false,
                verify: false,
                course_id: 1,
            },
        )
        .await
        .unwrap();
        assert_eq!(action, Action::Modified);
    }

    #[tokio::test]
    async fn move_remote_unchanged_changed_and_occupied() {
        let path = scratch();
        let dest = open_destination(&path, "id-1").unwrap();
        let remote = RemoteMeta {
            size: 4,
            updated_at: Some("t1".into()),
        };
        install_part_file(
            &dest,
            &FakeTransfer {
                files: std::collections::HashMap::from([(1, b"data".to_vec())]),
            },
            1,
            "old/name.txt",
            &remote,
            &InstallOpts {
                force: false,
                verify: false,
                course_id: 1,
            },
        )
        .await
        .unwrap();

        let lock_guard = dest
            .acquire_install_lock(std::time::Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(
            resolve_move(&dest, 1, "new/name.txt", &remote, false).unwrap(),
            MoveOutcome::Moved
        );
        drop(lock_guard);
        assert!(path.join("new/name.txt").exists());
        assert!(!path.join("old/name.txt").exists());

        // Move again with remote changed → download anew
        let remote2 = RemoteMeta {
            size: 4,
            updated_at: Some("t2".into()),
        };
        let lock_guard = dest
            .acquire_install_lock(std::time::Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(
            resolve_move(&dest, 1, "newer/name.txt", &remote2, false).unwrap(),
            MoveOutcome::DownloadAnew
        );
        drop(lock_guard);

        // Target occupied
        std::fs::create_dir_all(path.join("occ")).unwrap();
        std::fs::write(path.join("occ/taken.txt"), b"other").unwrap();
        // Re-install at a path we can move from
        install_part_file(
            &dest,
            &FakeTransfer {
                files: std::collections::HashMap::from([(2, b"zzzz".to_vec())]),
            },
            2,
            "src.txt",
            &RemoteMeta {
                size: 4,
                updated_at: Some("u".into()),
            },
            &InstallOpts {
                force: false,
                verify: false,
                course_id: 1,
            },
        )
        .await
        .unwrap();
        let lock_guard = dest
            .acquire_install_lock(std::time::Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(
            resolve_move(
                &dest,
                2,
                "occ/taken.txt",
                &RemoteMeta {
                    size: 4,
                    updated_at: Some("u".into()),
                },
                false
            )
            .unwrap(),
            MoveOutcome::DownloadAnew
        );
    }

    #[tokio::test]
    async fn pending_move_recovery_branches() {
        let path = scratch();
        let dest = open_destination(&path, "id-1").unwrap();
        let bytes = b"move";
        install_part_file(
            &dest,
            &FakeTransfer {
                files: std::collections::HashMap::from([(1, bytes.to_vec())]),
            },
            1,
            "a.txt",
            &RemoteMeta {
                size: 4,
                updated_at: Some("t".into()),
            },
            &InstallOpts {
                force: false,
                verify: false,
                course_id: 1,
            },
        )
        .await
        .unwrap();
        let sha = dest.manifest.get(1).unwrap().unwrap().sha256.unwrap();

        // only new matches
        std::fs::write(path.join("b.txt"), bytes).unwrap();
        std::fs::remove_file(path.join("a.txt")).unwrap();
        dest.manifest.set_pending_move(1, "b.txt", &sha).unwrap();
        // Fix path back to old for recovery semantics
        dest.manifest
            .upsert(&ManifestRow {
                file_id: 1,
                course_id: 1,
                path: "a.txt".into(),
                size: 4,
                sha256: Some(sha.clone()),
                remote_updated_at: Some("t".into()),
                installed_at: "t".into(),
                pending_move_to: Some("b.txt".into()),
                move_sha256: Some(sha.clone()),
            })
            .unwrap();
        let lock_guard = dest
            .acquire_install_lock(std::time::Duration::from_secs(5))
            .await
            .unwrap();
        let recovered = recover_pending_moves(&dest).unwrap();
        assert_eq!(recovered, vec![(1, Action::Moved)]);
        assert_eq!(dest.manifest.get(1).unwrap().unwrap().path, "b.txt");
        drop(lock_guard);

        // only old matches
        std::fs::write(path.join("old2.txt"), bytes).unwrap();
        dest.manifest
            .upsert(&ManifestRow {
                file_id: 2,
                course_id: 1,
                path: "old2.txt".into(),
                size: 4,
                sha256: Some(sha.clone()),
                remote_updated_at: Some("t".into()),
                installed_at: "t".into(),
                pending_move_to: Some("missing.txt".into()),
                move_sha256: Some(sha.clone()),
            })
            .unwrap();
        let lock_guard = dest
            .acquire_install_lock(std::time::Duration::from_secs(5))
            .await
            .unwrap();
        let recovered = recover_pending_moves(&dest).unwrap();
        assert!(recovered.contains(&(2, Action::Skipped)));
        drop(lock_guard);

        // neither matches
        dest.manifest
            .upsert(&ManifestRow {
                file_id: 3,
                course_id: 1,
                path: "gone.txt".into(),
                size: 4,
                sha256: Some(sha.clone()),
                remote_updated_at: Some("t".into()),
                installed_at: "t".into(),
                pending_move_to: Some("also-gone.txt".into()),
                move_sha256: Some(sha),
            })
            .unwrap();
        let lock_guard = dest
            .acquire_install_lock(std::time::Duration::from_secs(5))
            .await
            .unwrap();
        let recovered = recover_pending_moves(&dest).unwrap();
        assert!(recovered.contains(&(3, Action::UnresolvedMove)));
    }
}
