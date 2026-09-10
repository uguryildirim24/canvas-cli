//! Install critical section, clobber table, move protocol, transfer trait.

use crate::io::{CHANNEL_CAPACITY, CHUNK_SIZE, ChunkWriter};
use std::io::{Read, Seek, Write};
use tokio::io::{AsyncWrite, AsyncWriteExt};

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
    /// Preserve API classification without persisting response bodies or URLs.
    #[error(transparent)]
    Api(#[from] canvas_api::Error),
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
        sink: &mut (dyn AsyncWrite + Send + Unpin),
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
        sink: &mut (dyn AsyncWrite + Send + Unpin),
        expected_size: u64,
    ) -> Result<u64, TransferError> {
        let bytes = self
            .files
            .get(&file_id)
            .ok_or_else(|| TransferError::Message(format!("missing file {file_id}")))?;
        if bytes.len() as u64 != expected_size {
            return Err(TransferError::SizeMismatch);
        }
        sink.write_all(bytes).await?;
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
    #[error("blocking worker failed")]
    Worker,
}

impl InstallError {
    pub const fn action(&self) -> Action {
        match self {
            Self::Contain(ContainError::UnsafePath) | Self::Manifest(ManifestError::UnsafePath) => {
                Action::UnsafePath
            }
            _ => Action::Failed,
        }
    }
}

/// Completed-run precedence from §12.3/§14; dry-run never verifies files.
#[must_use]
pub fn outcome_exit_code(actions: &[Action], verify_mismatch: bool, dry_run: bool) -> u8 {
    if dry_run {
        0
    } else if verify_mismatch {
        10
    } else if outcome_is_partial(actions) {
        12
    } else {
        0
    }
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
        .filter(|&r| r.file_id == input.file_id && r.path == input.path);

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
    let lock = dest
        .acquire_install_lock(crate::download::manifest::LOCK_TIMEOUT)
        .await?;
    let d = dest.clone();
    let (path, metadata, options) = (rel_path.to_owned(), remote.clone(), opts.clone());
    let early = tokio::task::spawn_blocking(move || {
        let _lock = lock;
        if resolve_move_inner(&d, file_id, &path, &metadata, options.force)? == MoveOutcome::Moved {
            return Ok::<_, InstallError>(Some(Action::Moved));
        }
        match classify_at(&d, file_id, &path, &metadata, &options)? {
            ClobberDecision::Skip(action) => Ok(Some(action)),
            ClobberDecision::Install => Ok(None),
        }
    })
    .await
    .map_err(|_| InstallError::Worker)??;
    if let Some(action) = early {
        return Ok(action);
    }

    let root = dest.root.clone();
    let path = rel_path.to_owned();
    let part = tokio::task::spawn_blocking(move || PartFile::create(&root, &path))
        .await
        .map_err(|_| InstallError::Worker)??;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(CHANNEL_CAPACITY);
    let expected_size = remote.size;
    let worker = tokio::task::spawn_blocking(move || {
        let mut part = part;
        let mut hasher = Sha256::new();
        let mut written = 0u64;
        while let Some(chunk) = rx.blocking_recv() {
            if written + chunk.len() as u64 > expected_size {
                return Err(TransferError::SizeMismatch.into());
            }
            part.file
                .as_mut()
                .expect("part descriptor")
                .write_all(&chunk)?;
            hasher.update(&chunk);
            written += chunk.len() as u64;
        }
        part.file.as_ref().expect("part descriptor").sync_all()?;
        Ok::<_, InstallError>((part, written, hex_sha(&hasher.finalize())))
    });
    let mut sink = ChunkWriter::new(tx);
    let transfer_result = transfer.fetch(file_id, &mut sink, remote.size).await;
    drop(sink);
    let (part, actual, sha) = worker.await.map_err(|_| InstallError::Worker)??;
    let written = transfer_result?;
    if written != remote.size || actual != remote.size {
        return Err(TransferError::SizeMismatch.into());
    }

    let lock = dest
        .acquire_install_lock(crate::download::manifest::LOCK_TIMEOUT)
        .await?;
    let d = dest.clone();
    let (path, metadata, options) = (rel_path.to_owned(), remote.clone(), opts.clone());
    tokio::task::spawn_blocking(move || {
        let _lock = lock;
        let mut part = part;
        // Reuse the retained final parent for classification and rename.
        let fresh = walk_parent(&d.root, &path)?;
        if !same_parent(&fresh.parent, &part.contained.parent)? {
            return Err(ContainError::UnsafePath.into());
        }
        let decision = classify_at(&d, file_id, &path, &metadata, &options)?;
        if let ClobberDecision::Skip(action) = decision {
            return Ok(action);
        }
        install_rename(&part.contained.parent, &part.name, &part.contained.name)?;
        part.installed = true;
        d.manifest.upsert(&ManifestRow {
            file_id,
            course_id: options.course_id,
            path,
            size: actual,
            sha256: Some(sha),
            remote_updated_at: metadata.updated_at,
            installed_at: now_rfc3339(),
            pending_move_to: None,
            move_sha256: None,
        })?;
        Ok(Action::Downloaded)
    })
    .await
    .map_err(|_| InstallError::Worker)?
}

fn same_parent(a: &Dir, b: &Dir) -> Result<bool, std::io::Error> {
    use cap_std::fs::MetadataExt;
    let (a, b) = (a.dir_metadata()?, b.dir_metadata()?);
    #[cfg(unix)]
    return Ok(a.dev() == b.dev() && a.ino() == b.ino());
    #[cfg(windows)]
    return Ok(
        a.volume_serial_number() == b.volume_serial_number() && a.file_index() == b.file_index()
    );
}

struct PartFile {
    contained: std::sync::Arc<crate::download::contain::ContainedPath>,
    file: Option<cap_std::fs::File>,
    name: String,
    installed: bool,
}
impl PartFile {
    fn create(root: &Dir, path: &str) -> Result<Self, InstallError> {
        let contained = walk_parent(root, path)?;
        let (file, name) = create_part_file(
            &contained.parent,
            &contained.name,
            &crate::download::manifest::new_dest_id()?,
        )?;
        Ok(Self {
            contained: std::sync::Arc::new(contained),
            file: Some(file),
            name,
            installed: false,
        })
    }
}
impl Drop for PartFile {
    fn drop(&mut self) {
        if !self.installed {
            let (contained, name, file) =
                (self.contained.clone(), self.name.clone(), self.file.take());
            let cleanup = move || {
                drop(file);
                let _ = contained.parent.remove_file(&name);
            };
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                // Drop can run on the async thread when a transfer is cancelled.
                runtime.spawn_blocking(cleanup);
            } else {
                cleanup();
            }
        }
    }
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
        Ok(meta) if crate::download::contain::is_link(&meta) => {
            Err(ContainError::UnsafePath.into())
        }
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
    let mut buf = vec![0u8; CHUNK_SIZE];
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
pub async fn resolve_move(
    dest: &Destination,
    file_id: i64,
    new_path: &str,
    remote: &RemoteMeta,
    force: bool,
) -> Result<MoveOutcome, InstallError> {
    let lock = dest
        .acquire_install_lock(crate::download::manifest::LOCK_TIMEOUT)
        .await?;
    let (d, path, remote) = (dest.clone(), new_path.to_owned(), remote.clone());
    tokio::task::spawn_blocking(move || {
        let _lock = lock;
        resolve_move_inner(&d, file_id, &path, &remote, force)
    })
    .await
    .map_err(|_| InstallError::Worker)?
}

fn resolve_move_inner(
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

    let old = walk_parent(&dest.root, &row.path)?;
    let old_file = match open_contained_file(&old, false) {
        Ok(file) => file,
        Err(ContainError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MoveOutcome::DownloadAnew);
        }
        Err(e) => return Err(e.into()),
    };
    let old_hash = Some(hash_file_handle(&old_file)?);
    let hash_ok = match (&row.sha256, old_hash) {
        (Some(expected), Some(actual)) => expected == &actual,
        _ => false,
    };
    let remote_ok = remote_unchanged(&row, remote);

    let target = walk_parent(&dest.root, new_path)?;
    let (new_present, _, _) = inspect_local(&dest.root, new_path, false)?;
    let new_ok = !new_present || force;

    if hash_ok && remote_ok && new_ok {
        let sha = row.sha256.clone().unwrap_or_default();
        dest.manifest.set_pending_move(file_id, new_path, &sha)?;
        // Rename old → new through containment.
        old.parent.rename(&old.name, &target.parent, &target.name)?;
        super::contain::sync_dir(&target.parent)?;
        super::contain::sync_dir(&old.parent)?;
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

/// Recover `pending_move_to` markers at run start.
pub async fn recover_pending_moves(dest: &Destination) -> Result<Vec<(i64, Action)>, InstallError> {
    let lock = dest
        .acquire_install_lock(crate::download::manifest::LOCK_TIMEOUT)
        .await?;
    let d = dest.clone();
    tokio::task::spawn_blocking(move || {
        let _lock = lock;
        recover_pending_moves_inner(&d)
    })
    .await
    .map_err(|_| InstallError::Worker)?
}

pub(crate) fn recover_pending_moves_inner(
    dest: &Destination,
) -> Result<Vec<(i64, Action)>, InstallError> {
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
        let (old_hash, new_hash) = match (
            hash_path(&dest.root, &row.path),
            hash_path(&dest.root, &new_path),
        ) {
            (Ok(old), Ok(new)) => (old, new),
            (Err(InstallError::Contain(ContainError::UnsafePath)), _)
            | (_, Err(InstallError::Contain(ContainError::UnsafePath))) => {
                out.push((row.file_id, Action::UnsafePath));
                continue;
            }
            (Err(e), _) | (_, Err(e)) => return Err(e),
        };
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
                // Both actions refer to this file; the original row supplies the old path.
                out.push((row.file_id, Action::Moved));
                out.push((row.file_id, Action::Unmanaged));
            }
            (false, false) => {
                dest.manifest.clear_pending_move(row.file_id)?;
                out.push((row.file_id, Action::UnresolvedMove));
            }
        }
    }
    Ok(out)
}

fn hex_sha(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn now_rfc3339() -> String {
    jiff::Timestamp::now().to_string()
}

#[cfg(test)]
#[allow(clippy::used_underscore_binding, unused_variables)]
mod tests {
    use super::*;
    use crate::download::manifest::ManifestRow;
    use crate::download::manifest::test_support::open_destination;
    use crate::test_scratch::Scratch;

    fn scratch() -> Scratch {
        Scratch::new("canvas-core-install")
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
        assert_eq!(outcome_exit_code(&[Action::Failed], true, false), 10);
        assert_eq!(outcome_exit_code(&[Action::Failed], true, true), 0);
        assert_eq!(
            outcome_exit_code(&[Action::Modified, Action::Unmanaged], false, false),
            0
        );
        assert_eq!(outcome_exit_code(&[Action::Locked], false, false), 12);
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

        assert_eq!(
            resolve_move(&dest, 1, "new/name.txt", &remote, false)
                .await
                .unwrap(),
            MoveOutcome::Moved
        );
        assert!(path.join("new/name.txt").exists());
        assert!(!path.join("old/name.txt").exists());

        // Move again with remote changed → download anew
        let remote2 = RemoteMeta {
            size: 4,
            updated_at: Some("t2".into()),
        };
        assert_eq!(
            resolve_move(&dest, 1, "newer/name.txt", &remote2, false)
                .await
                .unwrap(),
            MoveOutcome::DownloadAnew
        );

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
            .await
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
        let recovered = recover_pending_moves(&dest).await.unwrap();
        assert_eq!(recovered, vec![(1, Action::Moved)]);
        assert_eq!(dest.manifest.get(1).unwrap().unwrap().path, "b.txt");

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
        let recovered = recover_pending_moves(&dest).await.unwrap();
        assert!(recovered.contains(&(2, Action::Skipped)));

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
        let recovered = recover_pending_moves(&dest).await.unwrap();
        assert!(recovered.contains(&(3, Action::UnresolvedMove)));
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::download::manifest::test_support::open_destination;
    use std::path::PathBuf;

    fn scratch() -> crate::test_scratch::Scratch {
        crate::test_scratch::Scratch::new("canvas-install-review")
    }
    fn remote() -> RemoteMeta {
        RemoteMeta {
            size: 4,
            updated_at: Some("t1".into()),
        }
    }
    fn opts() -> InstallOpts {
        InstallOpts {
            force: false,
            verify: true,
            course_id: 1,
        }
    }
    fn fake() -> FakeTransfer {
        FakeTransfer {
            files: std::collections::HashMap::from([(1, b"data".to_vec())]),
        }
    }
    fn parts(path: &std::path::Path) -> usize {
        std::fs::read_dir(path)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".part")
            })
            .count()
    }

    struct BadTransfer(bool);
    impl Transfer for BadTransfer {
        async fn fetch(
            &self,
            _: i64,
            sink: &mut (dyn AsyncWrite + Send + Unpin),
            _: u64,
        ) -> Result<u64, TransferError> {
            sink.write_all(b"bad").await?;
            if self.0 {
                Ok(4)
            } else {
                Err(TransferError::Message("injected".into()))
            }
        }
    }
    #[tokio::test]
    async fn failed_or_lying_transfer_never_installs_or_leaves_part() {
        let path = scratch();
        let dest = open_destination(&path, "A").unwrap();
        for lie in [false, true] {
            assert!(
                install_part_file(&dest, &BadTransfer(lie), 1, "f", &remote(), &opts())
                    .await
                    .is_err()
            );
            assert!(!path.join("f").exists());
            assert!(dest.manifest.get(1).unwrap().is_none());
            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                while parts(&path) != 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn integrated_move_force_and_crash_classification() {
        let path = scratch();
        let dest = open_destination(&path, "A").unwrap();
        assert_eq!(
            install_part_file(&dest, &fake(), 1, "old", &remote(), &opts())
                .await
                .unwrap(),
            Action::Downloaded
        );
        let empty = FakeTransfer {
            files: std::collections::HashMap::new(),
        };
        assert_eq!(
            install_part_file(&dest, &empty, 1, "new", &remote(), &opts())
                .await
                .unwrap(),
            Action::Moved
        );
        assert!(!path.join("old").exists());
        assert_eq!(std::fs::read(path.join("new")).unwrap(), b"data");
        // Equal-size replacement after rename but before DB commit.
        std::fs::write(path.join("new"), b"edit").unwrap();
        assert_eq!(
            install_part_file(&dest, &empty, 1, "new", &remote(), &opts())
                .await
                .unwrap(),
            Action::Modified
        );
        let mut options = opts();
        options.verify = false;
        assert_eq!(
            install_part_file(&dest, &empty, 1, "new", &remote(), &options)
                .await
                .unwrap(),
            Action::Skipped
        );
        options.force = true;
        options.verify = true;
        assert_eq!(
            install_part_file(&dest, &fake(), 1, "new", &remote(), &options)
                .await
                .unwrap(),
            Action::Downloaded
        );
        // New install after rename but before row commit.
        std::fs::write(path.join("uncommitted"), b"data").unwrap();
        assert_eq!(
            install_part_file(&dest, &empty, 2, "uncommitted", &remote(), &opts())
                .await
                .unwrap(),
            Action::Unmanaged
        );
    }

    #[tokio::test]
    async fn recovery_reports_both_and_rejects_same_size_wrong_target() {
        let path = scratch();
        let dest = open_destination(&path, "A").unwrap();
        install_part_file(&dest, &fake(), 1, "old", &remote(), &opts())
            .await
            .unwrap();
        let row = dest.manifest.get(1).unwrap().unwrap();
        std::fs::write(path.join("new"), b"data").unwrap();
        dest.manifest
            .set_pending_move(1, "new", row.sha256.as_ref().unwrap())
            .unwrap();
        let reopened = open_destination(&path, "A").unwrap();
        assert_eq!(
            reopened.recovery_actions,
            [(1, Action::Moved), (1, Action::Unmanaged)]
        );
        assert_eq!(reopened.manifest.get(1).unwrap().unwrap().path, "new");
        std::fs::write(path.join("wrong"), b"xxxx").unwrap();
        std::fs::remove_file(path.join("new")).unwrap();
        reopened
            .manifest
            .set_pending_move(1, "wrong", row.sha256.as_ref().unwrap())
            .unwrap();
        assert_eq!(
            recover_pending_moves(&reopened).await.unwrap(),
            [(1, Action::UnresolvedMove)]
        );
        assert_eq!(std::fs::read(path.join("wrong")).unwrap(), b"xxxx");
        assert_eq!(reopened.manifest.get(1).unwrap().unwrap().path, "new");
    }

    struct BarrierTransfer {
        path: PathBuf,
        label: String,
    }
    impl Transfer for BarrierTransfer {
        async fn fetch(
            &self,
            _: i64,
            sink: &mut (dyn AsyncWrite + Send + Unpin),
            _: u64,
        ) -> Result<u64, TransferError> {
            std::fs::write(self.path.join(format!("ready-{}", self.label)), b"ready")?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !self.path.join("go").exists() {
                assert!(std::time::Instant::now() < deadline, "barrier timeout");
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            sink.write_all(b"data").await?;
            Ok(4)
        }
    }

    #[tokio::test]
    async fn competing_child() {
        let Ok(base) = std::env::var("CANVAS_REVIEW_CHILD_BASE") else {
            return;
        };
        let label = std::env::var("CANVAS_REVIEW_CHILD_LABEL").unwrap();
        let base = PathBuf::from(base);
        let dest = open_destination(&base.join("dest"), "A").unwrap();
        let transfer = BarrierTransfer {
            path: base.clone(),
            label: label.clone(),
        };
        let action = install_part_file(&dest, &transfer, 1, "sub/f", &remote(), &opts())
            .await
            .unwrap();
        std::fs::write(base.join(format!("result-{label}")), action.as_str()).unwrap();
    }

    #[test]
    fn two_processes_reclassify_after_competing_transfers() {
        let base = scratch();
        open_destination(&base.join("dest"), "A").unwrap();
        let exe = std::env::current_exe().unwrap();
        let children: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|label| {
                std::process::Command::new(&exe)
                    .args([
                        "--exact",
                        "download::install::review_tests::competing_child",
                        "--nocapture",
                    ])
                    .env("CANVAS_REVIEW_CHILD_BASE", base.as_os_str())
                    .env("CANVAS_REVIEW_CHILD_LABEL", label)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !(base.join("ready-a").exists() && base.join("ready-b").exists()) {
            assert!(
                std::time::Instant::now() < deadline,
                "children did not both enter transfer"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        std::fs::write(base.join("go"), b"go").unwrap();
        for child in children {
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let mut actions: Vec<_> = ["a", "b"]
            .map(|label| std::fs::read_to_string(base.join(format!("result-{label}"))).unwrap())
            .into();
        actions.sort();
        assert_eq!(actions, ["downloaded", "skipped"]);
        let dest = open_destination(&base.join("dest"), "A").unwrap();
        assert_eq!(
            dest.manifest.get(1).unwrap().unwrap().sha256,
            hash_path(&dest.root, "sub/f").unwrap()
        );
        assert_eq!(parts(&base.join("dest/sub")), 0);
    }
    #[tokio::test]
    async fn forced_install_retires_previous_owner_of_target() {
        let path = scratch();
        let dest = open_destination(&path, "A").unwrap();
        install_part_file(&dest, &fake(), 1, "target", &remote(), &opts())
            .await
            .unwrap();
        let transfer = FakeTransfer {
            files: std::collections::HashMap::from([(2, b"next".to_vec())]),
        };
        let mut options = opts();
        options.force = true;
        install_part_file(&dest, &transfer, 2, "target", &remote(), &options)
            .await
            .unwrap();
        assert!(dest.manifest.get(1).unwrap().is_none());
        assert_eq!(dest.manifest.get(2).unwrap().unwrap().path, "target");
    }
}
