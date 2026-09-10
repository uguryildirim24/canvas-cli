//! Cross-process API slots (`<identity dir>/locks/api-slot-<n>.lock`).
//!
//! An admitted API request holds one slot file for its whole duration, so the
//! SPEC §11 concurrency cap is the same number whether one process or five are
//! running, and a process that dies frees its slot with its descriptor. The
//! files are created with `create_new` when absent and are never deleted.
//!
//! Storage transfers keep a per-process semaphore, as REPORT §3.6 says.

use std::fs::File;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use canvas_api::{Lane, LaneSlot, Permits};
use fs4::fs_std::FileExt;
use tokio::sync::Semaphore;
use tokio::time::{Instant, sleep};

/// Poll interval floor and ceiling while every slot is taken.
const POLL_MIN: Duration = Duration::from_millis(2);
const POLL_MAX: Duration = Duration::from_millis(40);

/// How long consecutive lock-file failures are tolerated before a request is
/// admitted without a cross-process slot. A broken lock directory must not
/// hang the command; it degrades to this process's own cap and says so.
const IO_GRACE: Duration = Duration::from_secs(5);

/// One held API slot. Dropping it releases the file lock.
struct SlotGuard {
    file: File,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

/// Slot acquisition backed by identity-local lock files.
pub struct FilePermits {
    slots: Vec<PathBuf>,
    storage: Arc<Semaphore>,
}

impl std::fmt::Debug for FilePermits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilePermits")
            .field("api_slots", &self.slots.len())
            .finish_non_exhaustive()
    }
}

impl FilePermits {
    /// Create the slot files under `locks_dir` if they are absent.
    pub fn new(
        locks_dir: &Path,
        api_concurrency: usize,
        storage_concurrency: usize,
    ) -> std::io::Result<Self> {
        std::fs::create_dir_all(locks_dir)?;
        let slots: Vec<PathBuf> = (0..api_concurrency.clamp(1, 8))
            .map(|n| locks_dir.join(format!("api-slot-{n}.lock")))
            .collect();
        for path in &slots {
            drop(open_lock_file(path)?);
        }
        Ok(Self {
            slots,
            storage: Arc::new(Semaphore::new(storage_concurrency.max(1))),
        })
    }

    /// Number of cross-process API slots.
    #[must_use]
    pub fn api_slots(&self) -> usize {
        self.slots.len()
    }

    /// Try each slot once, in order.
    fn try_slot(&self) -> std::io::Result<Option<SlotGuard>> {
        let mut last_error = None;
        for path in &self.slots {
            match open_lock_file(path).and_then(|file| {
                if FileExt::try_lock_exclusive(&file)? {
                    Ok(Some(SlotGuard { file }))
                } else {
                    Ok(None)
                }
            }) {
                Ok(Some(guard)) => return Ok(Some(guard)),
                Ok(None) => {}
                Err(e) => last_error = Some(e),
            }
        }
        match last_error {
            Some(e) => Err(e),
            None => Ok(None),
        }
    }
}

impl Permits for FilePermits {
    fn acquire(&self, lane: Lane) -> Pin<Box<dyn Future<Output = LaneSlot> + Send + '_>> {
        match lane {
            Lane::Storage => {
                let sem = Arc::clone(&self.storage);
                Box::pin(async move {
                    let permit = sem.acquire_owned().await.expect("storage semaphore");
                    Box::new(permit) as LaneSlot
                })
            }
            Lane::Api => Box::pin(async move {
                let mut wait = POLL_MIN;
                let mut failing_since: Option<Instant> = None;
                loop {
                    match self.try_slot() {
                        Ok(Some(guard)) => return Box::new(guard) as LaneSlot,
                        Ok(None) => failing_since = None,
                        Err(error) => {
                            let since = *failing_since.get_or_insert_with(Instant::now);
                            if since.elapsed() >= IO_GRACE {
                                tracing::warn!(
                                    %error,
                                    "cannot take a cross-process API slot; \
                                     capping requests in this process only"
                                );
                                return Box::new(()) as LaneSlot;
                            }
                        }
                    }
                    sleep(wait).await;
                    wait = (wait * 2).min(POLL_MAX);
                }
            }),
        }
    }
}

/// Open a lock file, creating it privately when it is absent.
///
/// `create_new` first, so two processes racing the first request cannot
/// truncate each other's file, and the fall-back open never follows a symlink
/// into anything but a regular file.
pub(crate) fn open_lock_file(path: &Path) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?,
        Err(e) => return Err(e),
    };
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
    }
    Ok(file)
}
