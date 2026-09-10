//! Credential store: keyring, fallback file, activation, and logout (§8).

#![allow(dead_code)] // backend helpers are used selectively per platform / command.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use fs4::fs_std::FileExt;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use canvas_core::identity::IdentityKey;
use canvas_core::store::{DbError, Store};

use crate::exit::CliError;
use crate::paths::CliPaths;

const SERVICE: &str = "canvas-cli";

/// Active credential source recorded in state DB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActiveSource {
    Keyring,
    File,
    None,
}

impl ActiveSource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keyring => "keyring",
            Self::File => "file",
            Self::None => "none",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "keyring" => Self::Keyring,
            "file" => Self::File,
            _ => Self::None,
        }
    }
}

/// Application credential errors. Never `Debug`-format `keyring::Error`.
#[derive(Debug, Error)]
pub enum CredError {
    #[error("credential not found")]
    NotFound,
    #[error("credential store access denied")]
    Denied,
    #[error("credential store locked")]
    Locked,
    #[error("credential backend unavailable: {0}")]
    Backend(String),
    #[error("unsafe credentials file: {0}")]
    Unsafe(String),
    #[error("{0}")]
    Io(String),
}

impl From<CredError> for CliError {
    fn from(value: CredError) -> Self {
        match value {
            CredError::NotFound => CliError::auth("no stored token; run auth login"),
            other => CliError::local(other.to_string()),
        }
    }
}

/// Map a keyring error without Debug-formatting secret-bearing variants.
pub fn map_keyring_error(err: keyring::Error) -> CredError {
    use keyring::Error;
    match &err {
        Error::NoEntry => CredError::NotFound,
        Error::NoStorageAccess(_) => CredError::Denied,
        Error::NoDefaultStore => {
            if Entry::store_status().is_err() {
                CredError::Backend("no default credential store".into())
            } else {
                CredError::Denied
            }
        }
        Error::PlatformFailure(inner) => {
            let msg = inner.to_string().to_ascii_lowercase();
            if msg.contains("lock") {
                CredError::Locked
            } else if msg.contains("denied")
                || msg.contains("permission")
                || msg.contains("auth")
                || msg.contains("access")
            {
                CredError::Denied
            } else {
                CredError::Backend(sanitize_keyring_message(&err))
            }
        }
        _ => CredError::Backend(sanitize_keyring_message(&err)),
    }
}

fn sanitize_keyring_message(err: &keyring::Error) -> String {
    // Prefer Display; never format Debug (BadEncoding carries secret bytes).
    let msg = err.to_string();
    if msg.len() > 200 {
        format!("{}…", &msg[..200])
    } else {
        msg
    }
}

/// Exclusive per-identity credential lock.
pub struct CredLock {
    _file: File,
}

impl CredLock {
    /// Take an exclusive lock on `<data>/locks/<key>.cred.lock`.
    pub fn acquire(paths: &CliPaths, key: &IdentityKey) -> Result<Self, CliError> {
        let path = paths.cred_lock(key.as_str());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| CliError::local(e.to_string()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| CliError::local(format!("cred lock {}: {e}", path.display())))?;
        FileExt::lock_exclusive(&file)
            .map_err(|e| CliError::local(format!("cred lock {}: {e}", path.display())))?;
        Ok(Self { _file: file })
    }
}

/// Hex SHA-256 of token bytes.
#[must_use]
pub fn token_sha256(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Whether tests force the file backend.
#[must_use]
pub fn force_file_store() -> bool {
    std::env::var_os("CANVAS_TEST_FORCE_FILE").is_some_and(|v| v != "0")
}

/// Whether the platform keyring store is available.
#[must_use]
pub fn keyring_available() -> bool {
    !force_file_store() && Entry::store_status().is_ok()
}

fn maybe_crash(step: &str) {
    if std::env::var("CANVAS_TEST_CRASH_AFTER").ok().as_deref() == Some(step) {
        eprintln!("CANVAS_TEST_CRASH_AFTER={step}");
        std::process::exit(99);
    }
}

/// Credential row snapshot from state DB.
#[derive(Debug, Clone)]
pub struct CredentialRow {
    pub active_source: ActiveSource,
    pub token_sha256: Option<String>,
    pub validated_at: Option<String>,
    pub cleanup_keyring: bool,
    pub cleanup_file: bool,
}

impl CredentialRow {
    pub fn load(store: &Store, key: &IdentityKey) -> Result<Self, CliError> {
        let key_s = key.as_str().to_owned();
        store
            .call_blocking(move |conns| {
                let mut stmt = conns.state.prepare(
                    "SELECT active_source, token_sha256, validated_at, cleanup_keyring, cleanup_file
                     FROM credential WHERE identity_key = ?1",
                )?;
                let row = stmt
                    .query_row(rusqlite::params![key_s], |r| {
                        Ok(CredentialRow {
                            active_source: ActiveSource::parse(&r.get::<_, String>(0)?),
                            token_sha256: r.get(1)?,
                            validated_at: r.get(2)?,
                            cleanup_keyring: r.get::<_, i64>(3)? != 0,
                            cleanup_file: r.get::<_, i64>(4)? != 0,
                        })
                    })
                    .optional()
                    .map_err(DbError::from)?;
                Ok(row.unwrap_or(CredentialRow {
                    active_source: ActiveSource::None,
                    token_sha256: None,
                    validated_at: None,
                    cleanup_keyring: false,
                    cleanup_file: false,
                }))
            })
            .map_err(CliError::from)
    }
}

trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Store a token under an identity key.
pub fn store_token(
    paths: &CliPaths,
    key: &IdentityKey,
    token: &str,
) -> Result<ActiveSource, CredError> {
    if keyring_available() {
        match Entry::new(SERVICE, key.as_str()) {
            Ok(entry) => match entry.set_password(token) {
                Ok(()) => return Ok(ActiveSource::Keyring),
                Err(e) => {
                    let mapped = map_keyring_error(e);
                    if !matches!(mapped, CredError::Backend(_)) {
                        return Err(mapped);
                    }
                    // Backend at login may fall back to file (non-Windows).
                }
            },
            Err(e) => {
                let mapped = map_keyring_error(e);
                if !matches!(mapped, CredError::Backend(_)) {
                    return Err(mapped);
                }
            }
        }
    }

    #[cfg(windows)]
    {
        let _ = paths;
        Err(CredError::Backend(
            "credential file fallback is not offered on Windows".into(),
        ))
    }
    #[cfg(not(windows))]
    {
        file_set(paths, key.as_str(), token)?;
        Ok(ActiveSource::File)
    }
}

/// Read a token from the named source.
pub fn read_token(
    paths: &CliPaths,
    key: &IdentityKey,
    source: ActiveSource,
) -> Result<String, CredError> {
    match source {
        ActiveSource::None => Err(CredError::NotFound),
        ActiveSource::Keyring => {
            let entry = Entry::new(SERVICE, key.as_str()).map_err(map_keyring_error)?;
            entry.get_password().map_err(map_keyring_error)
        }
        ActiveSource::File => {
            #[cfg(windows)]
            {
                let _ = (paths, key);
                Err(CredError::NotFound)
            }
            #[cfg(not(windows))]
            {
                file_get(paths, key.as_str())
            }
        }
    }
}

/// Delete a token from one store.
pub fn delete_token(
    paths: &CliPaths,
    key: &IdentityKey,
    source: ActiveSource,
) -> Result<(), CredError> {
    match source {
        ActiveSource::None => Ok(()),
        ActiveSource::Keyring => {
            let entry = Entry::new(SERVICE, key.as_str()).map_err(map_keyring_error)?;
            match entry.delete_credential() {
                Ok(()) => Ok(()),
                Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(map_keyring_error(e)),
            }
        }
        ActiveSource::File => {
            #[cfg(windows)]
            {
                let _ = (paths, key);
                Ok(())
            }
            #[cfg(not(windows))]
            {
                file_delete(paths, key.as_str())
            }
        }
    }
}

/// Activation protocol for `auth login`.
pub fn activate(
    paths: &CliPaths,
    store: &Store,
    key: &IdentityKey,
    token: &str,
    validated_at: &str,
) -> Result<ActiveSource, CliError> {
    let _lock = CredLock::acquire(paths, key)?;
    let source = store_token(paths, key, token)?;
    maybe_crash("after_store_write");

    let hash = token_sha256(token);
    let key_s = key.as_str().to_owned();
    let source_s = source.as_str().to_owned();
    let validated = validated_at.to_owned();
    let clear_keyring = source == ActiveSource::Keyring;
    let clear_file = source == ActiveSource::File;
    store
        .call_blocking(move |conns| {
            let tx = conns
                .state
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute(
                "INSERT INTO credential (
                    identity_key, active_source, token_sha256, validated_at,
                    cleanup_keyring, cleanup_file
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(identity_key) DO UPDATE SET
                    active_source = excluded.active_source,
                    token_sha256 = excluded.token_sha256,
                    validated_at = excluded.validated_at,
                    cleanup_keyring = CASE
                        WHEN excluded.active_source = 'keyring' THEN 0
                        ELSE 1
                    END,
                    cleanup_file = CASE
                        WHEN excluded.active_source = 'file' THEN 0
                        ELSE 1
                    END",
                rusqlite::params![
                    key_s,
                    source_s,
                    hash,
                    validated,
                    i64::from(!clear_keyring),
                    i64::from(!clear_file),
                ],
            )?;
            // Re-apply clear/set explicitly for clarity matching SPEC.
            if clear_keyring {
                tx.execute(
                    "UPDATE credential SET cleanup_keyring = 0, cleanup_file = 1
                     WHERE identity_key = ?1",
                    rusqlite::params![key_s],
                )?;
            } else if clear_file {
                tx.execute(
                    "UPDATE credential SET cleanup_file = 0, cleanup_keyring = 1
                     WHERE identity_key = ?1",
                    rusqlite::params![key_s],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
        .map_err(CliError::from)?;
    maybe_crash("after_state_commit");

    let _ = run_cleanups(paths, store, key);
    maybe_crash("after_cleanup");
    Ok(source)
}

/// Logout protocol.
pub fn logout(paths: &CliPaths, store: &Store, key: &IdentityKey) -> Result<Vec<String>, CliError> {
    let _lock = CredLock::acquire(paths, key)?;
    let row = CredentialRow::load(store, key)?;
    let mut cleanup_keyring = row.cleanup_keyring;
    let mut cleanup_file = row.cleanup_file;
    match row.active_source {
        ActiveSource::Keyring => cleanup_keyring = true,
        ActiveSource::File => cleanup_file = true,
        ActiveSource::None => {}
    }
    // Stray: present in the inactive store.
    if row.active_source != ActiveSource::Keyring && keyring_has(key) {
        cleanup_keyring = true;
    }
    #[cfg(not(windows))]
    if row.active_source != ActiveSource::File && file_has(paths, key.as_str()) {
        cleanup_file = true;
    }

    let key_s = key.as_str().to_owned();
    store
        .call_blocking(move |conns| {
            let tx = conns
                .state
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute(
                "INSERT INTO credential (
                    identity_key, active_source, token_sha256, validated_at,
                    cleanup_keyring, cleanup_file
                 ) VALUES (?1, 'none', NULL, NULL, ?2, ?3)
                 ON CONFLICT(identity_key) DO UPDATE SET
                    active_source = 'none',
                    token_sha256 = NULL,
                    validated_at = NULL,
                    cleanup_keyring = excluded.cleanup_keyring,
                    cleanup_file = excluded.cleanup_file",
                rusqlite::params![key_s, i64::from(cleanup_keyring), i64::from(cleanup_file)],
            )?;
            tx.commit()?;
            Ok(())
        })
        .map_err(CliError::from)?;
    maybe_crash("after_logout_state");

    let (removed, failures) = run_cleanups(paths, store, key)?;
    maybe_crash("after_logout_cleanup");
    if failures > 0 {
        return Err(CliError::local(
            "logout could not delete every credential store entry; cleanup flags kept",
        ));
    }
    Ok(removed)
}

fn keyring_has(key: &IdentityKey) -> bool {
    if !keyring_available() {
        return false;
    }
    match Entry::new(SERVICE, key.as_str()) {
        Ok(entry) => entry.get_password().is_ok(),
        Err(_) => false,
    }
}

fn run_cleanups(
    paths: &CliPaths,
    store: &Store,
    key: &IdentityKey,
) -> Result<(Vec<String>, usize), CliError> {
    let mut row = CredentialRow::load(store, key)?;
    let mut removed = Vec::new();
    let mut failures = 0usize;

    if row.cleanup_keyring {
        match delete_token(paths, key, ActiveSource::Keyring) {
            Ok(()) => {
                removed.push("keyring".into());
                row.cleanup_keyring = false;
                set_cleanup_flags(store, key, row.cleanup_keyring, row.cleanup_file)?;
            }
            Err(CredError::NotFound) => {
                row.cleanup_keyring = false;
                set_cleanup_flags(store, key, row.cleanup_keyring, row.cleanup_file)?;
            }
            Err(e) => {
                failures += 1;
                eprintln!("warning: keyring cleanup failed: {e}");
            }
        }
    }
    if row.cleanup_file {
        match delete_token(paths, key, ActiveSource::File) {
            Ok(()) => {
                removed.push("file".into());
                row.cleanup_file = false;
                set_cleanup_flags(store, key, row.cleanup_keyring, row.cleanup_file)?;
            }
            Err(CredError::NotFound) => {
                row.cleanup_file = false;
                set_cleanup_flags(store, key, row.cleanup_keyring, row.cleanup_file)?;
            }
            Err(e) => {
                failures += 1;
                eprintln!("warning: file cleanup failed: {e}");
            }
        }
    }
    Ok((removed, failures))
}

fn set_cleanup_flags(
    store: &Store,
    key: &IdentityKey,
    cleanup_keyring: bool,
    cleanup_file: bool,
) -> Result<(), CliError> {
    let key_s = key.as_str().to_owned();
    store
        .call_blocking(move |conns| {
            conns.state.execute(
                "UPDATE credential SET cleanup_keyring = ?2, cleanup_file = ?3
                 WHERE identity_key = ?1",
                rusqlite::params![key_s, i64::from(cleanup_keyring), i64::from(cleanup_file)],
            )?;
            Ok(())
        })
        .map_err(CliError::from)
}

/// Delete every store entry for identity removal.
pub fn delete_all_for_identity(paths: &CliPaths, key: &IdentityKey) -> Result<(), CliError> {
    let _lock = CredLock::acquire(paths, key)?;
    let mut errs = Vec::new();
    if let Err(e) = delete_token(paths, key, ActiveSource::Keyring) {
        if !matches!(e, CredError::NotFound | CredError::Backend(_)) {
            errs.push(e.to_string());
        }
    }
    #[cfg(not(windows))]
    if let Err(e) = delete_token(paths, key, ActiveSource::File) {
        if !matches!(e, CredError::NotFound) {
            errs.push(e.to_string());
        }
    }
    if !errs.is_empty() {
        return Err(CliError::local(format!(
            "failed to delete credentials: {}",
            errs.join("; ")
        )));
    }
    Ok(())
}

/// Report stray sources relative to the active source.
pub fn stray_sources(paths: &CliPaths, key: &IdentityKey, active: ActiveSource) -> Vec<String> {
    let mut out = Vec::new();
    if active != ActiveSource::Keyring && keyring_has(key) {
        out.push("keyring".into());
    }
    #[cfg(not(windows))]
    if active != ActiveSource::File && file_has(paths, key.as_str()) {
        out.push("file".into());
    }
    #[cfg(windows)]
    let _ = paths;
    out
}

// --- fallback file (Unix) ---------------------------------------------------

#[cfg(not(windows))]
#[derive(Debug, Default, Serialize, Deserialize)]
struct CredentialsFile {
    #[serde(default)]
    identities: BTreeMap<String, FileIdentity>,
}

#[cfg(not(windows))]
#[derive(Debug, Serialize, Deserialize)]
struct FileIdentity {
    token: String,
}

#[cfg(not(windows))]
fn file_has(paths: &CliPaths, key: &str) -> bool {
    match file_read_map(paths) {
        Ok(map) => map.identities.contains_key(key),
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn file_get(paths: &CliPaths, key: &str) -> Result<String, CredError> {
    let map = file_read_map(paths)?;
    map.identities
        .get(key)
        .map(|e| e.token.clone())
        .ok_or(CredError::NotFound)
}

#[cfg(not(windows))]
fn file_set(paths: &CliPaths, key: &str, token: &str) -> Result<(), CredError> {
    with_credentials_lock(paths, |map| {
        map.identities.insert(
            key.to_owned(),
            FileIdentity {
                token: token.to_owned(),
            },
        );
        Ok(())
    })
}

#[cfg(not(windows))]
fn file_delete(paths: &CliPaths, key: &str) -> Result<(), CredError> {
    with_credentials_lock(paths, |map| {
        map.identities.remove(key);
        Ok(())
    })
}

#[cfg(not(windows))]
fn with_credentials_lock(
    paths: &CliPaths,
    f: impl FnOnce(&mut CredentialsFile) -> Result<(), CredError>,
) -> Result<(), CredError> {
    fs::create_dir_all(&paths.config_dir).map_err(|e| CredError::Io(e.to_string()))?;
    let lock_path = paths.credentials_lock();
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| CredError::Io(e.to_string()))?;
    FileExt::lock_exclusive(&lock).map_err(|e| CredError::Io(e.to_string()))?;

    let mut map = match file_read_map(paths) {
        Ok(m) => m,
        Err(CredError::NotFound) => CredentialsFile::default(),
        Err(e) => return Err(e),
    };
    f(&mut map)?;
    file_write_map(paths, &map)?;
    Ok(())
}

#[cfg(not(windows))]
fn file_read_map(paths: &CliPaths) -> Result<CredentialsFile, CredError> {
    let path = paths.credentials_file();
    match open_nofollow_read(&path) {
        Ok(mut file) => {
            check_credentials_meta(&path, &file)?;
            let mut raw = String::new();
            file.read_to_string(&mut raw)
                .map_err(|e| CredError::Io(e.to_string()))?;
            if raw.trim().is_empty() {
                return Ok(CredentialsFile::default());
            }
            toml::from_str(&raw).map_err(|e| CredError::Io(e.to_string()))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(CredError::NotFound),
        Err(e) => Err(CredError::Io(e.to_string())),
    }
}

#[cfg(not(windows))]
fn file_write_map(paths: &CliPaths, map: &CredentialsFile) -> Result<(), CredError> {
    let path = paths.credentials_file();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = parent.join(format!(".credentials.{}.tmp", std::process::id()));
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| CredError::Io(e.to_string()))?;
        let raw = toml::to_string_pretty(map).map_err(|e| CredError::Io(e.to_string()))?;
        f.write_all(raw.as_bytes())
            .map_err(|e| CredError::Io(e.to_string()))?;
        f.sync_all().map_err(|e| CredError::Io(e.to_string()))?;
    }
    fs::rename(&tmp, &path).map_err(|e| CredError::Io(e.to_string()))?;
    sync_dir(parent)?;
    Ok(())
}

#[cfg(not(windows))]
fn open_nofollow_read(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    // O_NOFOLLOW: macOS/BSD 0x100, Linux 0x20000 (avoid `libc` + keep `unsafe_code = forbid`).
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    const O_NOFOLLOW: i32 = 0x0000_0100;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_NOFOLLOW: i32 = 0x0002_0000;
    OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(path)
}

#[cfg(not(windows))]
fn check_credentials_meta(path: &Path, file: &File) -> Result<(), CredError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let meta = file.metadata().map_err(|e| CredError::Io(e.to_string()))?;
    if !meta.file_type().is_file() {
        return Err(CredError::Unsafe(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    let mode = meta.mode() & 0o777;
    if mode != 0o600 {
        return Err(CredError::Unsafe(format!(
            "{} mode is {mode:o}, expected 0600",
            path.display()
        )));
    }
    // Compare ownership to a file we just created (no `geteuid`; `unsafe_code = forbid`).
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let probe = parent.join(format!(".canvas-cli-uid-probe.{}", std::process::id()));
    let probe_uid = {
        let probe_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&probe)
            .map_err(|e| CredError::Io(e.to_string()))?;
        let uid = probe_file
            .metadata()
            .map_err(|e| CredError::Io(e.to_string()))?
            .uid();
        drop(probe_file);
        let _ = fs::remove_file(&probe);
        uid
    };
    if meta.uid() != probe_uid {
        return Err(CredError::Unsafe(format!(
            "{} owner does not match the current user",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
fn sync_dir(dir: &Path) -> Result<(), CredError> {
    let f = File::open(dir).map_err(|e| CredError::Io(e.to_string()))?;
    f.sync_all().map_err(|e| CredError::Io(e.to_string()))
}

/// Backend label for status / login output.
#[must_use]
pub fn backend_label(source: ActiveSource) -> Option<&'static str> {
    match source {
        ActiveSource::Keyring => Some("keyring"),
        ActiveSource::File => Some("file"),
        ActiveSource::None => None,
    }
}

/// Path to the credentials file (for doctor messages).
#[must_use]
pub fn credentials_path(paths: &CliPaths) -> PathBuf {
    paths.credentials_file()
}
