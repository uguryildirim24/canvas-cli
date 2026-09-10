//! Per-identity coordinator (agent-UX REPORT §3.6).
//!
//! Every CLI, watch, and (later) MCP process that binds one identity shares
//! four things through the identity directory and `state.sqlite`:
//!
//! - **Request permits.** `locks/api-slot-<n>.lock`, one file per API slot, so
//!   the SPEC §11 concurrency cap holds across processes and a dead process
//!   frees its slot with its descriptor.
//! - **The governor row.** The §11 estimate, watermark, cooldown, and refill
//!   live in `state.sqlite` and are merged under `BEGIN IMMEDIATE`.
//! - **Refresh single-flight.** `locks/refresh-<dataset>-<scope>.lock` keeps
//!   two processes from fetching the same dataset scope at the same time.
//! - **Foreground interest.** A `submit` or `plan execute` registers before its
//!   first pre-flight request, and `watch` stops admitting polling work.
//!
//! None of these files is ever deleted; `identity remove` owns that (§3.4).
//! Storage transfers keep a per-process cap, as §3.6 says.

mod governor_state;
mod interest;
mod permits;
mod single_flight;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::identity::Paths;

pub use governor_state::SharedGovernorState;
pub use interest::{Interest, InterestKind};
pub use permits::FilePermits;
pub(crate) use permits::open_lock_file;
pub use single_flight::{RefreshAdmission, RefreshGuard, refresh_lock_name};

/// Coordinator failures.
#[derive(Debug, thiserror::Error)]
pub enum CoordError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

/// Default seconds a refresh waiter blocks before serving what it has (§3.6).
pub const REFRESH_WAIT_SECS: u64 = 30;

/// Coordinator tuning.
#[derive(Debug, Clone)]
pub struct CoordConfig {
    /// Cross-process API slots (`0 <= n < api_concurrency`).
    pub api_concurrency: usize,
    /// Per-process storage transfers.
    pub storage_concurrency: usize,
    /// How long a refresh waiter blocks before it serves what the cache has.
    pub refresh_wait: Duration,
}

impl Default for CoordConfig {
    fn default() -> Self {
        Self {
            api_concurrency: 4,
            storage_concurrency: 4,
            refresh_wait: Duration::from_secs(REFRESH_WAIT_SECS),
        }
    }
}

impl CoordConfig {
    /// The default configuration with the debug-only test overrides applied.
    #[must_use]
    pub fn from_env() -> Self {
        Self::default().with_test_overrides()
    }

    /// Apply `[network]` concurrency from the caller's configuration.
    #[must_use]
    pub fn with_concurrency(mut self, api: usize, storage: usize) -> Self {
        self.api_concurrency = api.clamp(1, 8);
        self.storage_concurrency = storage.max(1);
        self
    }

    /// Apply the debug-only test overrides.
    ///
    /// Two cross-process tests need a shorter waiter and a smaller cap than a
    /// real session, and they run the shipped binary, so the values have to
    /// arrive through the environment. Release builds ignore both.
    #[must_use]
    pub fn with_test_overrides(mut self) -> Self {
        if !cfg!(debug_assertions) {
            return self;
        }
        if let Some(n) = env_usize("CANVAS_TEST_API_CONCURRENCY") {
            self.api_concurrency = n.clamp(1, 8);
        }
        if let Some(ms) = env_millis("CANVAS_TEST_REFRESH_WAIT_MS") {
            self.refresh_wait = ms;
        }
        self
    }
}

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name).ok()?.parse().ok()
}

fn env_millis(name: &str) -> Option<Duration> {
    Some(Duration::from_millis(env_usize(name)?.try_into().ok()?))
}

/// The shared coordinator for one identity.
pub struct Coordinator {
    identity_dir: PathBuf,
    locks_dir: PathBuf,
    config: CoordConfig,
    conn: Arc<Mutex<Connection>>,
    permits: Arc<FilePermits>,
    governor: Arc<SharedGovernorState>,
}

impl std::fmt::Debug for Coordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Coordinator")
            .field("identity_dir", &self.identity_dir)
            .field("api_concurrency", &self.config.api_concurrency)
            .finish_non_exhaustive()
    }
}

impl Coordinator {
    /// Open the coordinator for an identity whose store already exists.
    ///
    /// The connection is this coordinator's own: the governor row is written
    /// on the request path, and the store's single worker thread must stay
    /// free for the command's own work.
    pub fn open(paths: &Paths, config: CoordConfig) -> Result<Self, CoordError> {
        let locks_dir = paths.identity_dir.join("locks");
        std::fs::create_dir_all(&locks_dir)?;
        let conn = Connection::open_with_flags(
            &paths.state_db,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let conn = Arc::new(Mutex::new(conn));
        let permits = Arc::new(FilePermits::new(
            &locks_dir,
            config.api_concurrency,
            config.storage_concurrency,
        )?);
        let governor = Arc::new(SharedGovernorState::new(Arc::clone(&conn)));
        Ok(Self {
            identity_dir: paths.identity_dir.clone(),
            locks_dir,
            config,
            conn,
            permits,
            governor,
        })
    }

    /// The seams to hand [`canvas_api::Client::with_seams`].
    #[must_use]
    pub fn seams(&self) -> canvas_api::Seams {
        canvas_api::Seams {
            permits: Some(Arc::clone(&self.permits) as Arc<dyn canvas_api::Permits>),
            state: Some(Arc::clone(&self.governor) as Arc<dyn canvas_api::GovernorState>),
        }
    }

    /// Coordinator tuning in force.
    #[must_use]
    pub fn config(&self) -> &CoordConfig {
        &self.config
    }

    /// The identity directory these locks live under.
    #[must_use]
    pub fn identity_dir(&self) -> &Path {
        &self.identity_dir
    }

    /// The lock directory (`<identity dir>/locks`).
    #[must_use]
    pub fn locks_dir(&self) -> &Path {
        &self.locks_dir
    }

    /// Take the single-flight lock for one dataset scope.
    ///
    /// Returns at once when the lock is free. Otherwise it waits up to
    /// [`CoordConfig::refresh_wait`]; the caller re-reads the cache when it
    /// waited, because the holder has probably just written what it wanted.
    pub async fn acquire_refresh(
        &self,
        dataset: &str,
        scope: &str,
    ) -> Result<RefreshAdmission, CoordError> {
        single_flight::acquire(&self.locks_dir, dataset, scope, self.config.refresh_wait).await
    }

    /// Register foreground submission interest for one assignment (§3.6).
    ///
    /// The returned handle deletes the row and releases the lock when dropped.
    /// `Ok(None)` means another live process already holds the interest for
    /// this assignment, which the §12.2 admission lock normally prevents.
    pub fn register_interest(
        &self,
        kind: InterestKind,
        assignment_id: i64,
    ) -> Result<Option<Interest>, CoordError> {
        interest::register(&self.locks_dir, Arc::clone(&self.conn), kind, assignment_id)
    }

    /// Whether any live foreground submission interest is registered.
    ///
    /// Rows whose registrant is gone are removed here, so a killed `submit`
    /// cannot starve polling.
    pub fn foreground_interest(&self) -> Result<bool, CoordError> {
        interest::any_live(&self.locks_dir, &self.conn)
    }
}
