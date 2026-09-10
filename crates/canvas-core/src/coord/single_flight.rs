//! Refresh single-flight (`<identity dir>/locks/refresh-<dataset>-<scope>.lock`).
//!
//! One process fetches a dataset scope; the others wait, then read what it
//! wrote. A waiter gives up after [`crate::coord::CoordConfig::refresh_wait`]
//! and serves the coverage the cache already holds, with honest §7 metadata.

use std::fs::File;
use std::path::Path;
use std::time::Duration;

use fs4::fs_std::FileExt;
use sha2::{Digest, Sha256};
use tokio::time::{Instant, sleep};

use super::CoordError;

/// Longest file name the encoder writes before it falls back to a digest.
const MAX_NAME: usize = 200;

/// Poll interval while another process holds the lock.
const POLL: Duration = Duration::from_millis(10);

/// Lower-case hex digits for the escape form.
const HEX: &[u8; 16] = b"0123456789abcdef";

/// A held refresh lock. Dropping it releases the file lock.
#[derive(Debug)]
pub struct RefreshGuard {
    file: File,
}

impl Drop for RefreshGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

/// How a refresh was admitted.
#[derive(Debug)]
pub enum RefreshAdmission {
    /// The lock was free: fetch.
    Fetch(RefreshGuard),
    /// Another process held it and has now finished: re-read the cache first.
    Waited(RefreshGuard),
    /// The holder is still working after the wait: serve what the cache has.
    TimedOut,
}

/// Canonical, filesystem-safe lock file name for one dataset scope.
///
/// Every byte outside `[a-z0-9._]` becomes `~<two lower-case hex digits>`,
/// including `~` itself and every upper-case letter. Two consequences matter:
/// the encoding is prefix-free, so no two distinct keys share a name, and the
/// result is entirely lower case, so a case-insensitive filesystem cannot fold
/// two scopes together either. The only `-` characters left are the two
/// separators, so `("a-b", "c")` and `("a", "b-c")` stay apart.
///
/// A name longer than 200 characters is replaced by a digest of the same two
/// components, which is injective short of a SHA-256 collision.
#[must_use]
pub fn refresh_lock_name(dataset: &str, scope: &str) -> String {
    let name = format!("refresh-{}-{}.lock", encode(dataset), encode(scope));
    if name.len() <= MAX_NAME {
        return name;
    }
    let mut hasher = Sha256::new();
    hasher.update(dataset.as_bytes());
    hasher.update([0u8]);
    hasher.update(scope.as_bytes());
    format!("refresh-{:x}.lock", hasher.finalize())
}

fn encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'_' {
            out.push(char::from(byte));
        } else {
            out.push('~');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    out
}

/// Take the single-flight lock, waiting at most `wait`.
pub(super) async fn acquire(
    locks_dir: &Path,
    dataset: &str,
    scope: &str,
    wait: Duration,
) -> Result<RefreshAdmission, CoordError> {
    let path = locks_dir.join(refresh_lock_name(dataset, scope));
    let file = super::open_lock_file(&path)?;
    if FileExt::try_lock_exclusive(&file)? {
        return Ok(RefreshAdmission::Fetch(RefreshGuard { file }));
    }
    let deadline = Instant::now() + wait;
    loop {
        sleep(POLL.min(wait)).await;
        let file = super::open_lock_file(&path)?;
        if FileExt::try_lock_exclusive(&file)? {
            return Ok(RefreshAdmission::Waited(RefreshGuard { file }));
        }
        if Instant::now() >= deadline {
            return Ok(RefreshAdmission::TimedOut);
        }
    }
}
