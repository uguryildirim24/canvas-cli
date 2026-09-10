//! Download destination metadata, install lock, and manifest DB.

use std::fs::File as StdFile;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use fs4::fs_std::FileExt;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{Mutex, MutexGuard};

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
    #[error("destination metadata is damaged")]
    DamagedDest,
    /// Identity mismatch.
    #[error("destination is bound to another identity")]
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
}

/// Opened destination: root handle, meta dir, manifest DB, locks.
pub struct Destination {
    /// Destination root (capability-scoped).
    pub root: Dir,
    /// Absolute path to the destination root.
    pub root_path: PathBuf,
    /// `.canvas-cli` directory handle.
    pub meta_dir: Dir,
    /// Parsed `dest.json`.
    pub dest: DestMeta,
    /// Manifest connection.
    pub manifest: Manifest,
    /// In-process install mutex.
    pub install_mutex: Mutex<()>,
    /// Path to `install.lock` (for reopening).
    lock_path: PathBuf,
}

/// Download-manifest database wrapper.
pub struct Manifest {
    conn: Connection,
}

impl Manifest {
    /// Open or create `<meta>/manifest.sqlite` and migrate.
    pub fn open(path: &Path) -> Result<Self, ManifestError> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;",
        )?;
        let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_USER_VERSION {
            return Err(ManifestError::NewerSchema { found: version });
        }
        if version < SCHEMA_USER_VERSION {
            conn.execute_batch("BEGIN IMMEDIATE;")?;
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS files (
                    file_id INTEGER PRIMARY KEY,
                    course_id INTEGER NOT NULL,
                    path TEXT NOT NULL,
                    size INTEGER NOT NULL,
                    sha256 TEXT,
                    remote_updated_at TEXT,
                    installed_at TEXT NOT NULL,
                    pending_move_to TEXT,
                    move_sha256 TEXT
                );
                PRAGMA user_version = 1;",
            )?;
            conn.execute_batch("COMMIT;")?;
        }
        Ok(Self { conn })
    }

    /// Read a row by file id.
    pub fn get(&self, file_id: i64) -> Result<Option<ManifestRow>, ManifestError> {
        self.conn
            .query_row(
                "SELECT file_id, course_id, path, size, sha256, remote_updated_at,
                        installed_at, pending_move_to, move_sha256
                 FROM files WHERE file_id = ?1",
                params![file_id],
                row_from,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Row whose `path` equals `path` (for clobber ownership checks).
    pub fn get_by_path(&self, path: &str) -> Result<Option<ManifestRow>, ManifestError> {
        self.conn
            .query_row(
                "SELECT file_id, course_id, path, size, sha256, remote_updated_at,
                        installed_at, pending_move_to, move_sha256
                 FROM files WHERE path = ?1",
                params![path],
                row_from,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Upsert a row (after rename).
    pub fn upsert(&self, row: &ManifestRow) -> Result<(), ManifestError> {
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        self.conn.execute(
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
                row.size.cast_signed(),
                row.sha256,
                row.remote_updated_at,
                row.installed_at,
                row.pending_move_to,
                row.move_sha256,
            ],
        )?;
        self.conn.execute_batch("COMMIT;")?;
        Ok(())
    }

    /// Set pending move marker.
    pub fn set_pending_move(
        &self,
        file_id: i64,
        pending_move_to: &str,
        move_sha256: &str,
    ) -> Result<(), ManifestError> {
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        self.conn.execute(
            "UPDATE files SET pending_move_to = ?1, move_sha256 = ?2 WHERE file_id = ?3",
            params![pending_move_to, move_sha256, file_id],
        )?;
        self.conn.execute_batch("COMMIT;")?;
        Ok(())
    }

    /// Finalize a move: set path, clear marker.
    pub fn finalize_move(&self, file_id: i64, new_path: &str) -> Result<(), ManifestError> {
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        self.conn.execute(
            "UPDATE files SET path = ?1, pending_move_to = NULL, move_sha256 = NULL WHERE file_id = ?2",
            params![new_path, file_id],
        )?;
        self.conn.execute_batch("COMMIT;")?;
        Ok(())
    }

    /// Clear pending move marker, keep old path.
    pub fn clear_pending_move(&self, file_id: i64) -> Result<(), ManifestError> {
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        self.conn.execute(
            "UPDATE files SET pending_move_to = NULL, move_sha256 = NULL WHERE file_id = ?1",
            params![file_id],
        )?;
        self.conn.execute_batch("COMMIT;")?;
        Ok(())
    }

    /// All rows with a pending move marker.
    pub fn pending_moves(&self) -> Result<Vec<ManifestRow>, ManifestError> {
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
        size: r.get::<_, i64>(3)?.cast_unsigned(),
        sha256: r.get(4)?,
        remote_updated_at: r.get(5)?,
        installed_at: r.get(6)?,
        pending_move_to: r.get(7)?,
        move_sha256: r.get(8)?,
    })
}

/// Held install lock (process + in-process).
#[derive(Debug)]
pub struct InstallLock<'a> {
    _guard: MutexGuard<'a, ()>,
    file: StdFile,
}

impl Drop for InstallLock<'_> {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

/// Default install-lock acquisition timeout.
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(30);

/// Open (or initialize) a download destination under `dest_path`.
pub fn open_destination(
    dest_path: &Path,
    identity_key: &str,
) -> Result<Destination, ManifestError> {
    std::fs::create_dir_all(dest_path)?;
    let root = Dir::open_ambient_dir(dest_path, ambient_authority())?;

    // Ensure `.canvas-cli` exists (create via root handle).
    match root.symlink_metadata(".canvas-cli") {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(std::io::Error::other("unsafe_path").into());
        }
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(std::io::Error::other("unsafe_path").into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            root.create_dir(".canvas-cli")?;
        }
        Err(e) => return Err(e.into()),
    }
    let meta_dir = root.open_dir_nofollow(".canvas-cli")?;

    let dest = read_or_create_dest_json(&meta_dir, identity_key)?;

    // Create install.lock if absent.
    ensure_install_lock(&meta_dir)?;

    let manifest_path = dest_path.join(".canvas-cli").join("manifest.sqlite");
    let manifest = Manifest::open(&manifest_path)?;

    Ok(Destination {
        root,
        root_path: dest_path.to_path_buf(),
        meta_dir,
        dest,
        manifest,
        install_mutex: Mutex::new(()),
        lock_path: dest_path.join(".canvas-cli").join("install.lock"),
    })
}

fn read_or_create_dest_json(meta_dir: &Dir, identity_key: &str) -> Result<DestMeta, ManifestError> {
    match meta_dir.open("dest.json") {
        Ok(mut f) => {
            let mut buf = String::new();
            f.read_to_string(&mut buf)?;
            if buf.trim().is_empty() {
                return Err(ManifestError::DamagedDest);
            }
            let meta: DestMeta =
                serde_json::from_str(&buf).map_err(|_| ManifestError::DamagedDest)?;
            if meta.format_version != FORMAT_VERSION {
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
                dest_id: new_dest_id(),
                identity_key: identity_key.to_string(),
                format_version: FORMAT_VERSION,
            };
            let bytes = serde_json::to_vec_pretty(&meta)?;
            let mut opts = OpenOptions::new();
            opts.write(true).create_new(true).follow(FollowSymlinks::No);
            let mut f = meta_dir.open_with("dest.json", &opts)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
            Ok(meta)
        }
        Err(e) => Err(e.into()),
    }
}

fn ensure_install_lock(meta_dir: &Dir) -> Result<(), ManifestError> {
    match meta_dir.symlink_metadata("install.lock") {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err(std::io::Error::other("unsafe_path").into())
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut opts = OpenOptions::new();
            opts.write(true).create_new(true).follow(FollowSymlinks::No);
            let f = meta_dir.open_with("install.lock", &opts)?;
            drop(f);
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

fn new_dest_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    // UUID-shaped id without depending on the uuid crate.
    format!("{nanos:032x}")
}

impl Destination {
    /// Acquire the install mutex and exclusive `install.lock` (timeout → `lock_timeout`).
    pub async fn acquire_install_lock(
        &self,
        timeout: Duration,
    ) -> Result<InstallLock<'_>, ManifestError> {
        let guard = self.install_mutex.lock().await;
        let deadline = Instant::now() + timeout;
        let file = StdFile::options()
            .read(true)
            .write(true)
            .open(&self.lock_path)?;
        loop {
            if FileExt::try_lock_exclusive(&file)? {
                return Ok(InstallLock {
                    _guard: guard,
                    file,
                });
            }
            if Instant::now() >= deadline {
                return Err(ManifestError::LockTimeout);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}
