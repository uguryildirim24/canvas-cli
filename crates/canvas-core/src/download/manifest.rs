//! Download destination metadata, install lock, and manifest DB.

use std::fs::File as StdFile;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::io::sqlite;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use fs4::fs_std::FileExt;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// `PRAGMA user_version` for migration `dl_0001`.
pub const SCHEMA_USER_VERSION: i32 = 1;

/// `dest.json` format version.
pub const FORMAT_VERSION: u32 = 1;

/// Destination metadata stored in `.canvas-cli/dest.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DestMeta {
    /// Destination uuid.
    pub dest_id: String,
    /// Bound identity key.
    pub identity_key: String,
    /// Format version.
    pub format_version: u32,
}

/// One manifest row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestRow {
    /// Canvas file id.
    pub file_id: i64,
    /// Course id.
    pub course_id: i64,
    /// Relative path under dest.
    pub path: String,
    /// Installed size.
    pub size: u64,
    /// Hex SHA-256 of installed bytes.
    pub sha256: Option<String>,
    /// Remote `updated_at` at install time.
    pub remote_updated_at: Option<String>,
    /// Install timestamp (RFC 3339).
    pub installed_at: String,
    /// Pending move target path.
    pub pending_move_to: Option<String>,
    /// Hash recorded for move recovery.
    pub move_sha256: Option<String>,
}

/// Manifest / destination errors.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// Damaged `dest.json`.
    #[error(
        "destination metadata is damaged; delete <dest>/.canvas-cli to start a new destination"
    )]
    DamagedDest,
    /// Identity mismatch.
    #[error("destination is bound to {bound}; choose another --dest or delete <dest>/.canvas-cli")]
    IdentityMismatch {
        /// Bound identity key.
        bound: String,
    },
    /// Newer schema than we support.
    #[error("manifest schema is newer than this CLI")]
    NewerSchema {
        /// Found `user_version`.
        found: i32,
    },
    /// Install lock not acquired within timeout.
    #[error("lock_timeout")]
    LockTimeout,
    /// I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Database error from rusqlite.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// JSON error.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("unsafe_path")]
    UnsafePath,
    #[error(
        "unregistered destination metadata; delete <dest>/.canvas-cli to start a new destination"
    )]
    Orphan,
    #[error(
        "this directory is not the registered destination {0}; delete <dest>/.canvas-cli to start a new destination; existing files are kept and treated as unmanaged"
    )]
    UnregisteredRoot(String),
    #[error("blocking worker failed")]
    Worker,
}

impl ManifestError {
    /// Initialization abort code; per-file lock failures are mapped to `failed` by install callers.
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::IdentityMismatch { .. } | Self::UnregisteredRoot(_) => 8,
            _ => 13,
        }
    }
}

/// Opened destination: root handle, meta dir, manifest DB, locks.
#[derive(Clone)]
pub struct Destination {
    /// Destination root (capability-scoped).
    pub root: Arc<Dir>,
    /// Absolute path to the destination root.
    pub root_path: PathBuf,
    /// `.canvas-cli` directory handle.
    pub meta_dir: Arc<Dir>,
    /// Parsed `dest.json`.
    pub dest: DestMeta,
    /// Manifest connection.
    pub manifest: Manifest,
    /// In-process install mutex.
    pub install_mutex: Arc<Mutex<()>>,
    /// Startup marker resolution, including unmanaged leftovers.
    pub recovery_actions: Vec<(i64, super::install::Action)>,
    /// Path to `install.lock` (for reopening).
    identity_downloads: Arc<Dir>,
}

/// Download-manifest database wrapper.
#[derive(Clone)]
pub struct Manifest {
    path: PathBuf,
}

struct ManifestConnection<'a> {
    conn: &'a mut Connection,
}

impl Manifest {
    /// Open the manifest in identity storage. Invoke from a blocking worker.
    pub fn open(path: &Path) -> Result<Self, ManifestError> {
        let version = sqlite::call(path, |conn| {
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
        })?;
        if version > SCHEMA_USER_VERSION {
            return Err(ManifestError::NewerSchema { found: version });
        }
        sqlite::call(path, |conn| {
            conn.execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA synchronous=FULL;",
            )?;
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS files (
                file_id INTEGER PRIMARY KEY, course_id INTEGER NOT NULL,
                path TEXT NOT NULL, size INTEGER NOT NULL CHECK(size >= 0),
                sha256 TEXT NOT NULL, remote_updated_at TEXT, installed_at TEXT NOT NULL,
                pending_move_to TEXT, move_sha256 TEXT); PRAGMA user_version=1;",
            )?;
            tx.commit()
        })?;
        Ok(Self {
            path: path.to_owned(),
        })
    }

    pub fn get(&self, file_id: i64) -> Result<Option<ManifestRow>, ManifestError> {
        sqlite::call(&self.path, move |conn| {
            ManifestConnection { conn }.get(file_id)
        })
        .map_err(Into::into)
    }
    pub fn get_by_path(&self, path: &str) -> Result<Option<ManifestRow>, ManifestError> {
        let path = path.to_owned();
        sqlite::call(&self.path, move |conn| {
            ManifestConnection { conn }.get_by_path(&path)
        })
        .map_err(Into::into)
    }
    pub fn upsert(&self, row: &ManifestRow) -> Result<(), ManifestError> {
        let row = row.clone();
        sqlite::call(&self.path, move |conn| {
            ManifestConnection { conn }.upsert(&row)
        })
        .map_err(Into::into)
    }
    pub fn set_pending_move(
        &self,
        file_id: i64,
        target: &str,
        sha: &str,
    ) -> Result<(), ManifestError> {
        let (target, sha) = (target.to_owned(), sha.to_owned());
        sqlite::call(&self.path, move |conn| {
            ManifestConnection { conn }.set_pending_move(file_id, &target, &sha)
        })
        .map_err(Into::into)
    }
    pub fn finalize_move(&self, file_id: i64, target: &str) -> Result<(), ManifestError> {
        let target = target.to_owned();
        sqlite::call(&self.path, move |conn| {
            ManifestConnection { conn }.finalize_move(file_id, &target)
        })
        .map_err(Into::into)
    }
    pub fn clear_pending_move(&self, file_id: i64) -> Result<(), ManifestError> {
        sqlite::call(&self.path, move |conn| {
            ManifestConnection { conn }.clear_pending_move(file_id)
        })
        .map_err(Into::into)
    }
    pub fn pending_moves(&self) -> Result<Vec<ManifestRow>, ManifestError> {
        sqlite::call(&self.path, move |conn| {
            ManifestConnection { conn }.pending_moves()
        })
        .map_err(Into::into)
    }
}

impl ManifestConnection<'_> {
    /// Read a row by file id.
    pub fn get(&self, file_id: i64) -> Result<Option<ManifestRow>, rusqlite::Error> {
        self.conn
            .query_row(
                "SELECT file_id, course_id, path, size, sha256, remote_updated_at,
                        installed_at, pending_move_to, move_sha256
                 FROM files WHERE file_id = ?1",
                params![file_id],
                row_from,
            )
            .optional()
    }

    /// Row whose `path` equals `path` (for clobber ownership checks).
    pub fn get_by_path(&self, path: &str) -> Result<Option<ManifestRow>, rusqlite::Error> {
        self.conn
            .query_row(
                "SELECT file_id, course_id, path, size, sha256, remote_updated_at,
                        installed_at, pending_move_to, move_sha256
                 FROM files WHERE path = ?1",
                params![path],
                row_from,
            )
            .optional()
    }

    /// Upsert a row (after rename).
    pub fn upsert(&mut self, row: &ManifestRow) -> Result<(), rusqlite::Error> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM files WHERE path=?1 AND file_id<>?2",
            params![row.path, row.file_id],
        )?;
        tx.execute(
            "INSERT INTO files (
                file_id, course_id, path, size, sha256, remote_updated_at,
                installed_at, pending_move_to, move_sha256
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(file_id) DO UPDATE SET
                course_id=excluded.course_id,
                path=excluded.path,
                size=excluded.size,
                sha256=excluded.sha256,
                remote_updated_at=excluded.remote_updated_at,
                installed_at=excluded.installed_at,
                pending_move_to=excluded.pending_move_to,
                move_sha256=excluded.move_sha256",
            params![
                row.file_id,
                row.course_id,
                row.path,
                i64::try_from(row.size).map_err(|_| rusqlite::Error::InvalidQuery)?,
                row.sha256,
                row.remote_updated_at,
                row.installed_at,
                row.pending_move_to,
                row.move_sha256,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Set pending move marker.
    pub fn set_pending_move(
        &mut self,
        file_id: i64,
        pending_move_to: &str,
        move_sha256: &str,
    ) -> Result<(), rusqlite::Error> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE files SET pending_move_to = ?1, move_sha256 = ?2 WHERE file_id = ?3",
            params![pending_move_to, move_sha256, file_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Finalize a move: set path, clear marker.
    pub fn finalize_move(&mut self, file_id: i64, new_path: &str) -> Result<(), rusqlite::Error> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM files WHERE path=?1 AND file_id<>?2",
            params![new_path, file_id],
        )?;
        tx.execute(
            "UPDATE files SET path = ?1, pending_move_to = NULL, move_sha256 = NULL WHERE file_id = ?2",
            params![new_path, file_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Clear pending move marker, keep old path.
    pub fn clear_pending_move(&mut self, file_id: i64) -> Result<(), rusqlite::Error> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE files SET pending_move_to = NULL, move_sha256 = NULL WHERE file_id = ?1",
            params![file_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// All rows with a pending move marker.
    pub fn pending_moves(&self) -> Result<Vec<ManifestRow>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT file_id, course_id, path, size, sha256, remote_updated_at,
                    installed_at, pending_move_to, move_sha256
             FROM files WHERE pending_move_to IS NOT NULL",
        )?;
        let rows = stmt
            .query_map([], row_from)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<ManifestRow> {
    Ok(ManifestRow {
        file_id: r.get(0)?,
        course_id: r.get(1)?,
        path: r.get(2)?,
        size: u64::try_from(r.get::<_, i64>(3)?)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(3, -1))?,
        sha256: r.get(4)?,
        remote_updated_at: r.get(5)?,
        installed_at: r.get(6)?,
        pending_move_to: r.get(7)?,
        move_sha256: r.get(8)?,
    })
}

/// Registry record owned by the state-store migration, not the manifest migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DestinationRecord {
    pub dest_id: String,
    pub canonical_path: String,
    pub root_fingerprint: String,
    pub created_at: String,
}

/// State-store integration: implementations access `destinations` on the shared
/// `SQLite` worker and use an immediate transaction for `put`. Called under both locks.
pub trait DestinationRegistry: Send + Sync {
    fn get(&self, dest_id: &str) -> Result<Option<DestinationRecord>, ManifestError>;
    fn put(&self, record: DestinationRecord) -> Result<(), ManifestError>;
}

/// Adapter for the state store's already-migrated `destinations` table.
/// This module never creates or migrates `state.sqlite`; that remains the store owner's job.
pub struct SqliteDestinationRegistry {
    state_path: PathBuf,
}
impl SqliteDestinationRegistry {
    pub fn from_migrated_state(state_path: &Path) -> Result<Self, ManifestError> {
        if !state_path.is_file() {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
        }
        Ok(Self {
            state_path: state_path.to_owned(),
        })
    }
}
impl DestinationRegistry for SqliteDestinationRegistry {
    fn get(&self, dest_id: &str) -> Result<Option<DestinationRecord>, ManifestError> {
        let id = dest_id.to_owned();
        sqlite::call(&self.state_path, move |conn| {
            conn.query_row("SELECT dest_id, canonical_path, root_fingerprint, created_at FROM destinations WHERE dest_id=?1", [id], |r| Ok(DestinationRecord {
                dest_id: r.get(0)?, canonical_path: r.get(1)?, root_fingerprint: r.get(2)?, created_at: r.get(3)?,
            })).optional()
        }).map_err(Into::into)
    }
    fn put(&self, row: DestinationRecord) -> Result<(), ManifestError> {
        sqlite::call(&self.state_path, move |conn| {
            conn.busy_timeout(Duration::from_secs(5))?;
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute("INSERT INTO destinations (dest_id, canonical_path, root_fingerprint, created_at) VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(dest_id) DO UPDATE SET canonical_path=excluded.canonical_path",
                params![row.dest_id, row.canonical_path, row.root_fingerprint, row.created_at])?;
            tx.commit()
        }).map_err(Into::into)
    }
}

/// Held install locks (root, then identity). Dropping releases in reverse order.
#[derive(Debug)]
pub struct InstallLock {
    _guard: OwnedMutexGuard<()>,
    _identity: FileLock,
    _root: FileLock,
}
#[derive(Debug)]
struct FileLock(StdFile);
impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

pub const LOCK_TIMEOUT: Duration = Duration::from_secs(30);

/// Initialize on a blocking worker with a verified identity directory and its registry.
/// The caller keeps the identity lifetime lock required by §10.
pub fn open_destination(
    dest_path: &Path,
    identity_key: &str,
    identity_dir: &Path,
    registry: &dyn DestinationRegistry,
) -> Result<Destination, ManifestError> {
    std::fs::create_dir_all(dest_path)?;
    let root = Arc::new(Dir::open_ambient_dir(dest_path, ambient_authority())?);
    match root.create_dir(".canvas-cli") {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    if super::contain::is_link(&root.symlink_metadata(".canvas-cli")?) {
        return Err(ManifestError::UnsafePath);
    }
    let meta_dir = Arc::new(
        root.open_dir_nofollow(".canvas-cli")
            .map_err(|_| ManifestError::UnsafePath)?,
    );
    let deadline = Instant::now() + LOCK_TIMEOUT;
    let root_lock = lock_until(open_lock(&meta_dir, "install.lock")?, deadline)?;
    let dest = read_or_create_dest_json(&meta_dir, identity_key)?;
    // No identity-side write or lock before validating existing metadata.
    let downloads_path = identity_dir.join("downloads");
    std::fs::create_dir_all(&downloads_path)?;
    let identity_downloads = Arc::new(Dir::open_ambient_dir(&downloads_path, ambient_authority())?);
    let identity_lock = lock_until(
        open_lock(&identity_downloads, &format!("{}.lock", dest.dest_id))?,
        deadline,
    )?;
    let manifest_path = downloads_path.join(format!("{}.sqlite", dest.dest_id));
    let canonical_path = std::fs::canonicalize(dest_path)?
        .to_string_lossy()
        .into_owned();
    let fingerprint = root_fingerprint(&root)?;
    match registry.get(&dest.dest_id)? {
        None if manifest_path.try_exists()? => return Err(ManifestError::Orphan),
        None => registry.put(DestinationRecord {
            dest_id: dest.dest_id.clone(),
            canonical_path,
            root_fingerprint: fingerprint,
            created_at: jiff::Timestamp::now().to_string(),
        })?,
        Some(mut row) => {
            if row.root_fingerprint != fingerprint {
                return Err(ManifestError::UnregisteredRoot(row.canonical_path));
            }
            if row.canonical_path != canonical_path {
                row.canonical_path = canonical_path;
                registry.put(row)?;
            }
        }
    }
    let manifest = Manifest::open(&manifest_path)?;
    let mut destination = Destination {
        root,
        root_path: dest_path.to_owned(),
        meta_dir,
        dest,
        manifest,
        install_mutex: Arc::new(Mutex::new(())),
        identity_downloads,
        recovery_actions: Vec::new(),
    };
    destination.recovery_actions = super::install::recover_pending_moves_inner(&destination)
        .map_err(|e| match e {
            super::install::InstallError::Manifest(e) => e,
            _ => ManifestError::UnsafePath,
        })?;
    drop(identity_lock);
    drop(root_lock);
    Ok(destination)
}

fn root_fingerprint(root: &Dir) -> Result<String, ManifestError> {
    use cap_std::fs::MetadataExt;
    let meta = root.dir_metadata()?;
    #[cfg(unix)]
    return Ok(format!("{}:{}", meta.dev(), meta.ino()));
    #[cfg(windows)]
    return Ok(format!(
        "{}:{}",
        meta.volume_serial_number()
            .ok_or(ManifestError::UnsafePath)?,
        meta.file_index().ok_or(ManifestError::UnsafePath)?
    ));
}

fn read_or_create_dest_json(meta_dir: &Dir, identity_key: &str) -> Result<DestMeta, ManifestError> {
    match meta_dir.symlink_metadata("dest.json") {
        Ok(m) if super::contain::is_link(&m) || !m.is_file() => {
            return Err(ManifestError::UnsafePath);
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut opts = OpenOptions::new();
    opts.read(true).follow(FollowSymlinks::No);
    match meta_dir.open_with("dest.json", &opts) {
        Ok(mut f) => {
            if !f.metadata()?.is_file() {
                return Err(ManifestError::UnsafePath);
            }
            let mut buf = String::new();
            f.read_to_string(&mut buf)
                .map_err(|_| ManifestError::DamagedDest)?;
            let meta: DestMeta =
                serde_json::from_str(&buf).map_err(|_| ManifestError::DamagedDest)?;
            if meta.format_version != FORMAT_VERSION || !valid_dest_id(&meta.dest_id) {
                return Err(ManifestError::DamagedDest);
            }
            if meta.identity_key != identity_key {
                return Err(ManifestError::IdentityMismatch {
                    bound: meta.identity_key,
                });
            }
            Ok(meta)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let meta = DestMeta {
                dest_id: new_dest_id()?,
                identity_key: identity_key.to_owned(),
                format_version: FORMAT_VERSION,
            };
            let mut opts = OpenOptions::new();
            opts.write(true).create_new(true).follow(FollowSymlinks::No);
            let mut f = meta_dir.open_with("dest.json", &opts)?;
            f.write_all(&serde_json::to_vec_pretty(&meta)?)?;
            f.sync_all()?;
            super::contain::sync_dir(meta_dir)?;
            Ok(meta)
        }
        Err(_) => Err(ManifestError::UnsafePath),
    }
}

fn valid_dest_id(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

pub(crate) fn new_dest_id() -> Result<String, std::io::Error> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(std::io::Error::other)?;
    b[6] = (b[6] & 15) | 64;
    b[8] = (b[8] & 63) | 128;
    let mut h = String::new();
    for v in b {
        use std::fmt::Write as _;
        write!(&mut h, "{v:02x}").expect("format UUID");
    }
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    ))
}

fn open_lock(dir: &Dir, name: &str) -> Result<StdFile, ManifestError> {
    match dir.symlink_metadata(name) {
        Ok(m) if super::contain::is_link(&m) || !m.is_file() => {
            return Err(ManifestError::UnsafePath);
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut opts = OpenOptions::new();
    opts.read(true)
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let file = match dir.open_with(name, &opts) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            opts.create_new(false);
            dir.open_with(name, &opts)
                .map_err(|_| ManifestError::UnsafePath)?
        }
        Err(e) => return Err(e.into()),
    };
    if !file.metadata()?.is_file() {
        return Err(ManifestError::UnsafePath);
    }
    Ok(file.into_std())
}

fn lock_until(file: StdFile, deadline: Instant) -> Result<FileLock, ManifestError> {
    loop {
        if FileExt::try_lock_exclusive(&file)? {
            return Ok(FileLock(file));
        }
        if Instant::now() >= deadline {
            return Err(ManifestError::LockTimeout);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

impl Destination {
    /// Total timeout includes the in-process mutex and both file locks.
    pub async fn acquire_install_lock(
        &self,
        timeout: Duration,
    ) -> Result<InstallLock, ManifestError> {
        let deadline = Instant::now() + timeout;
        let guard = tokio::time::timeout(timeout, self.install_mutex.clone().lock_owned())
            .await
            .map_err(|_| ManifestError::LockTimeout)?;
        let root = self.meta_dir.clone();
        let identity = self.identity_downloads.clone();
        let name = format!("{}.lock", self.dest.dest_id);
        let (root_lock, identity_lock) = tokio::task::spawn_blocking(move || {
            let root = lock_until(open_lock(&root, "install.lock")?, deadline)?;
            let identity = lock_until(open_lock(&identity, &name)?, deadline)?;
            Ok::<_, ManifestError>((root, identity))
        })
        .await
        .map_err(|_| ManifestError::Worker)??;
        Ok(InstallLock {
            _guard: guard,
            _identity: identity_lock,
            _root: root_lock,
        })
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    pub struct Registry(pub PathBuf);
    impl DestinationRegistry for Registry {
        fn get(&self, id: &str) -> Result<Option<DestinationRecord>, ManifestError> {
            let p = self.0.join(format!("{id}.json"));
            if !p.exists() {
                return Ok(None);
            }
            Ok(Some(serde_json::from_slice(&std::fs::read(p)?)?))
        }
        fn put(&self, row: DestinationRecord) -> Result<(), ManifestError> {
            std::fs::create_dir_all(&self.0)?;
            std::fs::write(
                self.0.join(format!("{}.json", row.dest_id)),
                serde_json::to_vec(&row)?,
            )?;
            Ok(())
        }
    }
    pub fn open_destination(path: &Path, key: &str) -> Result<Destination, ManifestError> {
        let identity = path.with_extension("identity");
        super::open_destination(path, key, &identity, &Registry(identity.join("registry")))
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::Registry;
    use super::*;

    fn scratch() -> PathBuf {
        let p = std::env::temp_dir().join(format!("canvas-destination-{}", new_dest_id().unwrap()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn registered_root_rename_copy_orphan_and_identity() {
        let base = scratch();
        let (path, identity) = (base.join("dest"), base.join("identity"));
        let registry = Registry(identity.join("registry"));
        let dest = open_destination(&path, "A", &identity, &registry).unwrap();
        let id = dest.dest.dest_id.clone();
        assert!(valid_dest_id(&id));
        assert!(
            identity
                .join("downloads")
                .join(format!("{id}.sqlite"))
                .exists()
        );
        let mut names: Vec<_> = std::fs::read_dir(path.join(".canvas-cli"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, ["dest.json", "install.lock"]);
        drop(dest);
        let moved = base.join("moved");
        std::fs::rename(&path, &moved).unwrap();
        let dest = open_destination(&moved, "A", &identity, &registry).unwrap();
        assert_eq!(dest.dest.dest_id, id);
        assert_eq!(
            registry.get(&id).unwrap().unwrap().canonical_path,
            std::fs::canonicalize(&moved).unwrap().to_string_lossy()
        );
        let copied = base.join("copy");
        std::fs::create_dir_all(copied.join(".canvas-cli")).unwrap();
        std::fs::copy(
            moved.join(".canvas-cli/dest.json"),
            copied.join(".canvas-cli/dest.json"),
        )
        .unwrap();
        assert!(matches!(
            open_destination(&copied, "A", &identity, &registry),
            Err(ManifestError::UnregisteredRoot(_))
        ));
        let identity_b = base.join("B");
        assert!(matches!(
            open_destination(
                &moved,
                "B",
                &identity_b,
                &Registry(identity_b.join("registry"))
            ),
            Err(ManifestError::IdentityMismatch { .. })
        ));
        assert!(!identity_b.exists());
        std::fs::remove_file(registry.0.join(format!("{id}.json"))).unwrap();
        assert!(matches!(
            open_destination(&moved, "A", &identity, &registry),
            Err(ManifestError::Orphan)
        ));
    }

    #[test]
    fn damaged_metadata_and_crash_before_registration() {
        let base = scratch();
        let path = base.join("dest");
        let identity = base.join("identity");
        let registry = Registry(identity.join("registry"));
        std::fs::create_dir_all(path.join(".canvas-cli")).unwrap();
        for data in [
            "",
            "{",
            r#"{"dest_id":"../../escape","identity_key":"A","format_version":1}"#,
        ] {
            std::fs::write(path.join(".canvas-cli/dest.json"), data).unwrap();
            assert!(matches!(
                open_destination(&path, "A", &identity, &registry),
                Err(ManifestError::DamagedDest)
            ));
            assert!(!identity.exists());
        }
        let meta = DestMeta {
            dest_id: new_dest_id().unwrap(),
            identity_key: "A".into(),
            format_version: 1,
        };
        std::fs::write(
            path.join(".canvas-cli/dest.json"),
            serde_json::to_vec(&meta).unwrap(),
        )
        .unwrap();
        let dest = open_destination(&path, "A", &identity, &registry).unwrap();
        assert_eq!(dest.dest, meta);
        assert!(registry.get(&meta.dest_id).unwrap().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn destination_alias_allowed_but_metadata_and_lock_symlinks_refused() {
        let base = scratch();
        let path = base.join("dest");
        let identity = base.join("identity");
        let registry = Registry(identity.join("registry"));
        let dest = open_destination(&path, "A", &identity, &registry).unwrap();
        let alias = base.join("alias");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert_eq!(
            open_destination(&alias, "A", &identity, &registry)
                .unwrap()
                .dest,
            dest.dest
        );
        for name in ["dest.json", "install.lock"] {
            let metadata = path.join(".canvas-cli");
            std::fs::rename(metadata.join(name), metadata.join("saved")).unwrap();
            std::os::unix::fs::symlink("saved", metadata.join(name)).unwrap();
            assert!(matches!(
                open_destination(&path, "A", &identity, &registry),
                Err(ManifestError::UnsafePath)
            ));
            std::fs::remove_file(metadata.join(name)).unwrap();
            std::fs::rename(metadata.join("saved"), metadata.join(name)).unwrap();
        }
    }

    #[test]
    fn newer_schema_and_failed_transaction() {
        let base = scratch();
        let newer = base.join("newer.sqlite");
        sqlite::call(&newer, |conn| conn.execute_batch("PRAGMA user_version=2")).unwrap();
        assert!(matches!(
            Manifest::open(&newer),
            Err(ManifestError::NewerSchema { found: 2 })
        ));
        let manifest = Manifest::open(&base.join("manifest.sqlite")).unwrap();
        let mut row = ManifestRow {
            file_id: 1,
            course_id: 1,
            path: "f".into(),
            size: u64::MAX,
            sha256: Some("abc".into()),
            remote_updated_at: None,
            installed_at: "t".into(),
            pending_move_to: None,
            move_sha256: None,
        };
        assert!(manifest.upsert(&row).is_err());
        row.size = 3;
        manifest.upsert(&row).unwrap();
        assert_eq!(manifest.get(1).unwrap(), Some(row));
    }

    #[tokio::test]
    async fn in_process_timeout_and_retained_root_lock() {
        let base = scratch();
        let dest = test_support::open_destination(&base.join("dest"), "A").unwrap();
        let held = dest
            .acquire_install_lock(Duration::from_secs(1))
            .await
            .unwrap();
        assert!(matches!(
            dest.acquire_install_lock(Duration::from_millis(20)).await,
            Err(ManifestError::LockTimeout)
        ));
        drop(held);
        std::fs::rename(base.join("dest"), base.join("renamed")).unwrap();
        let _held = dest
            .acquire_install_lock(Duration::from_secs(1))
            .await
            .unwrap();
    }
    #[test]
    fn registry_adapter_uses_existing_state_schema() {
        let base = scratch();
        let state = base.join("state.sqlite");
        assert!(SqliteDestinationRegistry::from_migrated_state(&state).is_err());
        assert!(!state.exists());
        sqlite::call(&state, |conn| conn.execute_batch("CREATE TABLE destinations (dest_id TEXT PRIMARY KEY, canonical_path TEXT NOT NULL, root_fingerprint TEXT NOT NULL, created_at TEXT NOT NULL)")).unwrap();
        let registry = SqliteDestinationRegistry::from_migrated_state(&state).unwrap();
        let row = DestinationRecord {
            dest_id: new_dest_id().unwrap(),
            canonical_path: "first".into(),
            root_fingerprint: "1:2".into(),
            created_at: "now".into(),
        };
        registry.put(row.clone()).unwrap();
        let mut moved = row.clone();
        moved.canonical_path = "second".into();
        registry.put(moved).unwrap();
        let read = registry.get(&row.dest_id).unwrap().unwrap();
        assert_eq!(read.canonical_path, "second");
        assert_eq!(read.root_fingerprint, row.root_fingerprint);
    }

    #[tokio::test]
    async fn different_roots_same_id_share_identity_lock() {
        let base = scratch();
        let a = test_support::open_destination(&base.join("a"), "A").unwrap();
        let mut b = test_support::open_destination(&base.join("b"), "A").unwrap();
        // Exercise the lock primitive independently of the copied-root refusal.
        b.dest = a.dest.clone();
        b.identity_downloads = a.identity_downloads.clone();
        let held = a
            .acquire_install_lock(Duration::from_secs(1))
            .await
            .unwrap();
        assert!(matches!(
            b.acquire_install_lock(Duration::from_millis(30)).await,
            Err(ManifestError::LockTimeout)
        ));
        drop(held);
        let _held = b
            .acquire_install_lock(Duration::from_secs(1))
            .await
            .unwrap();
    }
}
