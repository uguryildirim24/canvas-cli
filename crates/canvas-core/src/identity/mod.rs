//! Identity key, `identity.json`, and identity lock protocol.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

/// Concrete paths the CLI builds and passes into core.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Data root (`~/.local/share/canvas-cli/` or Windows equivalent).
    pub data_root: PathBuf,
    /// Identity directory under the data root.
    pub identity_dir: PathBuf,
    /// Identity lock file: `<data root>/locks/<key>.lock`.
    pub lock_path: PathBuf,
    /// Cache database path.
    pub cache_db: PathBuf,
    /// State database path.
    pub state_db: PathBuf,
}

impl Paths {
    /// Build paths for an identity key under `data_root`.
    #[must_use]
    pub fn for_identity(data_root: impl Into<PathBuf>, key: &IdentityKey) -> Self {
        let data_root = data_root.into();
        let identity_dir = data_root.join(key.as_str());
        let locks = data_root.join("locks");
        Self {
            lock_path: locks.join(format!("{}.lock", key.as_str())),
            cache_db: identity_dir.join("cache.sqlite"),
            state_db: identity_dir.join("state.sqlite"),
            identity_dir,
            data_root,
        }
    }

    /// Enforce the identity layout before opening or removing any files.
    pub fn verify(&self, key: &IdentityKey) -> Result<(), IdentityError> {
        if key.as_str().is_empty()
            || !key
                .as_str()
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        {
            return Err(IdentityError::Mismatch {
                reason: "unsafe identity key".into(),
            });
        }
        let expected = Self::for_identity(&self.data_root, key);
        if self.identity_dir != expected.identity_dir
            || self.lock_path != expected.lock_path
            || self.cache_db != expected.cache_db
            || self.state_db != expected.state_db
        {
            return Err(IdentityError::Mismatch {
                reason: "paths do not belong to identity".into(),
            });
        }
        for path in [
            self.data_root.clone(),
            self.data_root.join("locks"),
            self.identity_dir.clone(),
            self.lock_path.clone(),
            self.identity_json(),
            self.cache_db.clone(),
            self.state_db.clone(),
        ] {
            reject_symlink(&path)?;
        }
        for db in [&self.cache_db, &self.state_db] {
            for suffix in ["-wal", "-shm", "-journal"] {
                let mut path = db.as_os_str().to_os_string();
                path.push(suffix);
                reject_symlink(Path::new(&path))?;
            }
        }
        Ok(())
    }

    /// Path to `identity.json` inside the identity directory.
    #[must_use]
    pub fn identity_json(&self) -> PathBuf {
        self.identity_dir.join("identity.json")
    }
}

/// Filesystem-safe identity key: `<host-slug>[_<port>]-<user_id>-<digest8>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IdentityKey(String);

impl IdentityKey {
    /// Compute the key from a canonical origin and Canvas user id.
    #[must_use]
    pub fn compute(origin: &str, user_id: i64) -> Self {
        let (host, port) = parse_origin_host_port(origin);
        let host_slug: String = host
            .chars()
            .map(|c| {
                if matches!(c, 'a'..='z' | '0'..='9' | '.' | '-') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let digest8 = {
            let mut hasher = Sha256::new();
            hasher.update(origin.as_bytes());
            hasher.update(b"\n");
            hasher.update(user_id.to_string().as_bytes());
            let full = hasher.finalize();
            hex::encode_n(&full[..4])
        };
        let key = if let Some(port) = port {
            format!("{host_slug}_{port}-{user_id}-{digest8}")
        } else {
            format!("{host_slug}-{user_id}-{digest8}")
        };
        Self(key)
    }

    /// Parse a filesystem-safe identity key operand (for `identity remove`).
    pub fn parse(raw: &str) -> Result<Self, IdentityError> {
        if raw.is_empty()
            || matches!(raw, "." | "..")
            || !raw
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        {
            return Err(IdentityError::Mismatch {
                reason: "unsafe identity key".into(),
            });
        }
        Ok(Self(raw.to_owned()))
    }

    /// Borrow the key string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for IdentityKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for IdentityKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Contents of `identity.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityDocument {
    pub origin: String,
    pub user_id: i64,
    pub key: IdentityKey,
    pub created_at: String,
    pub generation: Uuid,
}

impl IdentityDocument {
    /// Create a new document with a fresh generation uuid.
    #[must_use]
    pub fn new(origin: impl Into<String>, user_id: i64, created_at: impl Into<String>) -> Self {
        let origin = origin.into();
        let key = IdentityKey::compute(&origin, user_id);
        Self {
            origin,
            user_id,
            key,
            created_at: created_at.into(),
            generation: Uuid::new_v4(),
        }
    }

    /// Verify origin, user id, and key consistency.
    pub fn verify(&self) -> Result<(), IdentityError> {
        let expected = IdentityKey::compute(&self.origin, self.user_id);
        if self.key != expected {
            return Err(IdentityError::Mismatch {
                reason: "identity key does not match origin and user id".into(),
            });
        }
        Ok(())
    }

    /// Read and deserialize `identity.json`.
    pub fn read(path: &Path) -> Result<Self, IdentityError> {
        let raw = fs::read_to_string(path).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                IdentityError::Changed
            } else {
                IdentityError::Io(e)
            }
        })?;
        let doc: Self = serde_json::from_str(&raw)?;
        doc.verify()?;
        Ok(doc)
    }

    /// Write `identity.json` with mode `0600` on Unix.
    pub fn write(&self, path: &Path) -> Result<(), IdentityError> {
        self.verify()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_vec_pretty(self)?;
        atomic_write(path, &raw)?;
        Ok(())
    }
}

/// Held shared or exclusive identity lock.
#[derive(Debug)]
pub struct IdentityLock {
    _file: File,
    kind: LockKind,
    path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LockKind {
    Shared,
    Exclusive,
}

impl IdentityLock {
    /// Take a shared lock for the lifetime of an opener. Re-verifies after acquire.
    pub fn acquire_shared(
        paths: &Paths,
        expected: &IdentityDocument,
    ) -> Result<Self, IdentityError> {
        expected.verify()?;
        paths.verify(&expected.key)?;
        ensure_lock_file(paths)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&paths.lock_path)?;
        // Prefer fs4 over std::fs::File::lock_* (MSRV 1.89).
        FileExt::lock_shared(&file).map_err(IdentityError::Io)?;
        let lock = Self {
            _file: file,
            kind: LockKind::Shared,
            path: paths.lock_path.clone(),
        };
        reverify_after_acquire(paths, expected)?;
        Ok(lock)
    }

    /// Take an exclusive lock with a 5 s timeout (removal protocol).
    pub fn acquire_exclusive(paths: &Paths) -> Result<Self, IdentityError> {
        let doc = IdentityDocument::read(&paths.identity_json())?;
        paths.verify(&doc.key)?;
        ensure_lock_file(paths)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&paths.lock_path)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match FileExt::try_lock_exclusive(&file) {
                Ok(true) => {
                    return Ok(Self {
                        _file: file,
                        kind: LockKind::Exclusive,
                        path: paths.lock_path.clone(),
                    });
                }
                Ok(false) => {
                    if Instant::now() >= deadline {
                        return Err(IdentityError::LockTimeout);
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(IdentityError::Io(e)),
            }
        }
    }

    /// Whether this lock is exclusive.
    #[must_use]
    pub fn is_exclusive(&self) -> bool {
        self.kind == LockKind::Exclusive
    }
}

/// Callbacks used by the identity removal protocol.
pub struct RemovalCallbacks<'a> {
    /// Delete credential entries for this identity. Failure aborts removal.
    pub delete_credentials: &'a mut dyn FnMut() -> Result<(), IdentityError>,
    /// Remove profiles that reference the key and clear `default_profile` if needed.
    pub remove_profiles: &'a mut dyn FnMut() -> Result<(), IdentityError>,
}

/// Run the removal protocol under an exclusive identity lock.
///
/// Order: credentials → identity directory → profiles. Releases by dropping `lock`.
pub fn remove_identity(
    paths: &Paths,
    lock: IdentityLock,
    callbacks: &mut RemovalCallbacks<'_>,
) -> Result<(), IdentityError> {
    if !lock.is_exclusive() || lock.path != paths.lock_path {
        return Err(IdentityError::Mismatch {
            reason: "identity removal requires an exclusive lock".into(),
        });
    }
    // Re-read before destructive work.
    let doc = IdentityDocument::read(&paths.identity_json())?;
    paths.verify(&doc.key)?;
    (callbacks.delete_credentials)()?;
    if paths.identity_dir.exists() {
        fs::remove_dir_all(&paths.identity_dir)?;
    }
    (callbacks.remove_profiles)()?;
    drop(lock);
    Ok(())
}

/// Errors from identity path, lock, and document operations.
#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("identity changed")]
    Changed,
    #[error("identity lock timed out")]
    LockTimeout,
    #[error("identity mismatch: {reason}")]
    Mismatch { reason: String },
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

fn ensure_lock_file(paths: &Paths) -> Result<(), IdentityError> {
    if let Some(parent) = paths.lock_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if !paths.lock_path.exists() {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&paths.lock_path)
            .or_else(|e| {
                if e.kind() == io::ErrorKind::AlreadyExists {
                    OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&paths.lock_path)
                } else {
                    Err(e)
                }
            })?;
        let _ = f.write_all(b"canvas-cli identity lock\n");
        let _ = f.sync_all();
    }
    Ok(())
}

fn reverify_after_acquire(paths: &Paths, expected: &IdentityDocument) -> Result<(), IdentityError> {
    if !paths.identity_dir.exists() {
        return Err(IdentityError::Changed);
    }
    let doc = IdentityDocument::read(&paths.identity_json())?;
    if doc.generation != expected.generation
        || doc.key != expected.key
        || doc.origin != expected.origin
        || doc.user_id != expected.user_id
    {
        return Err(IdentityError::Changed);
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), IdentityError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(IdentityError::Mismatch {
            reason: "identity paths must not be symbolic links".into(),
        }),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = parent.join(format!(".identity-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn parse_origin_host_port(origin: &str) -> (String, Option<u16>) {
    let rest = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .unwrap_or(origin);
    // Keep brackets in the IPv6 host so the slug matches §8 (`[::1]` → `___1_`).
    if rest.starts_with('[')
        && let Some(end) = rest.find(']')
    {
        let host = rest[..=end].to_string();
        let after = &rest[end + 1..];
        let port = after
            .strip_prefix(':')
            .and_then(|p| p.parse().ok())
            .filter(|&p: &u16| p != 443);
        return (host, port);
    }
    if let Some((host, port_s)) = rest.rsplit_once(':')
        && let Ok(port) = port_s.parse::<u16>()
    {
        if port != 443 {
            return (host.to_string(), Some(port));
        }
        return (host.to_string(), None);
    }
    (rest.to_string(), None)
}

/// Tiny hex helper so we do not pull in the `hex` crate.
mod hex {
    pub fn encode_n(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0xf) as usize] as char);
        }
        out
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;

    #[test]
    fn identity_key_examples() {
        let k = IdentityKey::compute("https://canvas.example.edu", 12345);
        assert!(k.as_str().starts_with("canvas.example.edu-12345-"));
        assert_eq!(k.as_str().len(), "canvas.example.edu-12345-".len() + 8);

        let k6 = IdentityKey::compute("https://[::1]:8443", 7);
        assert!(k6.as_str().starts_with("___1__8443-7-"));
    }
}
