//! Dedicated `SQLite` thread and async `call` handle.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, OnceLock};
use std::thread;

use rusqlite::{Connection, OpenFlags};
use thiserror::Error;

use crate::coord::{CoordConfig, CoordError, Coordinator};
use crate::identity::{IdentityDocument, IdentityError, IdentityLock, Paths};

use super::migrate::{self, CACHE_USER_VERSION, STATE_USER_VERSION};

const CHANNEL_BOUND: usize = 64;

type Job = Box<dyn FnOnce(&mut HashMap<u64, StoreConns>) + Send>;

/// Open cache and state connections owned by the `SQLite` thread.
pub struct StoreConns {
    /// Disposable cache database.
    pub cache: Connection,
    /// Durable state database.
    pub state: Connection,
    // Drop connections before releasing the identity lock, including cancelled jobs.
    _lock: Arc<IdentityLock>,
}

/// Handle to the process-wide `SQLite` worker.
pub struct Store {
    tx: SyncSender<Job>,
    id: u64,
    pub(super) lock: Arc<IdentityLock>,
    coord: Arc<Coordinator>,
}

static WORKER: OnceLock<SyncSender<Job>> = OnceLock::new();
static NEXT_STORE: AtomicU64 = AtomicU64::new(1);

fn worker() -> &'static SyncSender<Job> {
    WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Job>(CHANNEL_BOUND);
        thread::Builder::new()
            .name("canvas-sqlite".into())
            .spawn(move || sqlite_thread(&rx))
            .expect("cannot start SQLite worker");
        tx
    })
}

/// Run download-manifest work on the same bounded worker as cache/state.
pub(crate) fn auxiliary_sqlite<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, rusqlite::Error> + Send + 'static,
) -> Result<T, rusqlite::Error> {
    let (tx, rx) = mpsc::sync_channel(1);
    worker()
        .send(Box::new(move |_| {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
                .unwrap_or(Err(rusqlite::Error::InvalidQuery));
            let _ = tx.send(result);
        }))
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    rx.recv().map_err(|_| rusqlite::Error::InvalidQuery)?
}

/// Database open / call errors.
#[derive(Debug, Error)]
pub enum DbError {
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(
        "database schema is newer than this binary (user_version={found}, supported={supported})"
    )]
    NewerSchema { found: i32, supported: i32 },
    #[error("sqlite worker queue is closed")]
    WorkerClosed,
    #[error("sqlite worker panicked")]
    WorkerPanicked,
    #[error("{0}")]
    Message(String),
}

impl From<CoordError> for DbError {
    fn from(value: CoordError) -> Self {
        match value {
            CoordError::Io(e) => Self::Identity(IdentityError::Io(e)),
            CoordError::Sqlite(e) => Self::Sqlite(e),
        }
    }
}

impl Store {
    /// Open `cache.sqlite` and `state.sqlite` on a dedicated thread.
    pub fn open(paths: &Paths, identity: &IdentityDocument) -> Result<Self, DbError> {
        Self::open_with_coord(paths, identity, CoordConfig::from_env())
    }

    /// Open with explicit coordinator tuning (`[network] api_concurrency`).
    pub fn open_with_coord(
        paths: &Paths,
        identity: &IdentityDocument,
        coord: CoordConfig,
    ) -> Result<Self, DbError> {
        identity.verify()?;
        paths.verify(&identity.key)?;
        let lock = Arc::new(IdentityLock::acquire_shared(paths, identity)?);
        let coord_paths = paths.clone();
        let paths = paths.clone();
        let doc = identity.clone();
        let db_lock = Arc::clone(&lock);
        let id = NEXT_STORE.fetch_add(1, Ordering::Relaxed);
        let tx = worker().clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        tx.send(Box::new(move |stores| {
            let result = (|| {
                let cache = open_db(&paths.cache_db, CACHE_USER_VERSION, migrate::migrate_cache)?;
                let mut state =
                    open_db(&paths.state_db, STATE_USER_VERSION, migrate::migrate_state)?;
                let state_tx =
                    state.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                for (key, value) in [
                    ("origin", doc.origin),
                    ("user_id", doc.user_id.to_string()),
                    ("key", doc.key.to_string()),
                    ("created_at", doc.created_at),
                    ("generation", doc.generation.to_string()),
                ] {
                    state_tx.execute(
                        "INSERT INTO identity (key, value) VALUES (?1, ?2)
                        ON CONFLICT(key) DO NOTHING",
                        rusqlite::params![key, value],
                    )?;
                    let stored: String = state_tx.query_row(
                        "SELECT value FROM identity WHERE key = ?1",
                        [key],
                        |r| r.get(0),
                    )?;
                    if stored != value {
                        return Err(DbError::Identity(IdentityError::Changed));
                    }
                }
                state_tx.commit()?;
                Ok(StoreConns {
                    cache,
                    state,
                    _lock: db_lock,
                })
            })();
            match result {
                Ok(conns) => {
                    stores.insert(id, conns);
                    if ready_tx.send(Ok(())).is_err() {
                        stores.remove(&id);
                    }
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        }))
        .map_err(|_| DbError::WorkerClosed)?;
        ready_rx.recv().map_err(|_| DbError::WorkerClosed)??;
        // The coordinator needs the migrated `governor` and `interest` tables,
        // so it opens after the store thread reports both databases ready.
        let coord = Arc::new(Coordinator::open(&coord_paths, coord)?);
        Ok(Self {
            tx,
            id,
            lock,
            coord,
        })
    }

    /// The per-identity coordinator (permits, governor row, single-flight).
    #[must_use]
    pub fn coordinator(&self) -> &Arc<Coordinator> {
        &self.coord
    }

    /// Run `f` on the `SQLite` thread and return its result.
    pub async fn call<F, T>(&self, f: F) -> Result<T, DbError>
    where
        F: FnOnce(&mut StoreConns) -> Result<T, DbError> + Send + 'static,
        T: Send + 'static,
    {
        let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
        let id = self.id;
        let job: Job = Box::new(move |stores| {
            let result = stores
                .get_mut(&id)
                .ok_or(DbError::WorkerClosed)
                .and_then(|conns| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(conns)))
                        .unwrap_or(Err(DbError::WorkerPanicked))
                });
            let _ = resp_tx.send(result);
        });
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || tx.send(job))
            .await
            .map_err(|_| DbError::WorkerPanicked)?
            .map_err(|_| DbError::WorkerClosed)?;
        match resp_rx.await {
            Ok(r) => r,
            Err(_) => Err(DbError::WorkerPanicked),
        }
    }

    /// Blocking variant of [`Self::call`] for tests and sync callers.
    pub fn call_blocking<F, T>(&self, f: F) -> Result<T, DbError>
    where
        F: FnOnce(&mut StoreConns) -> Result<T, DbError> + Send + 'static,
        T: Send + 'static,
    {
        let (resp_tx, resp_rx) = mpsc::sync_channel(1);
        let id = self.id;
        let job: Job = Box::new(move |stores| {
            let result = stores
                .get_mut(&id)
                .ok_or(DbError::WorkerClosed)
                .and_then(|conns| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(conns)))
                        .unwrap_or(Err(DbError::WorkerPanicked))
                });
            let _ = resp_tx.send(result);
        });
        self.tx.send(job).map_err(|_| DbError::WorkerClosed)?;
        resp_rx.recv().map_err(|_| DbError::WorkerPanicked)?
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let id = self.id;
        let job: Job = Box::new(move |stores| {
            stores.remove(&id);
        });
        if let Err(mpsc::TrySendError::Full(job)) = self.tx.try_send(job) {
            let tx = self.tx.clone();
            // Never block the current-thread runtime when the bounded queue is full.
            thread::spawn(move || {
                let _ = tx.send(job);
            });
        }
    }
}

fn sqlite_thread(rx: &Receiver<Job>) {
    let mut stores = HashMap::new();
    while let Ok(job) = rx.recv() {
        job(&mut stores);
    }
}

fn open_db(
    path: &Path,
    supported: i32,
    migrate: fn(&Connection, i32) -> Result<(), DbError>,
) -> Result<Connection, DbError> {
    // Create privately before SQLite opens the database or its journal sidecars.
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(DbError::Identity(IdentityError::Io(e))),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(IdentityError::Io)?;
    }
    let mut conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    enable_wal(&conn)?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    let found: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if found > supported {
        return Err(DbError::NewerSchema { found, supported });
    }
    if found < supported {
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        // Another process may have migrated while this one waited for the writer lock.
        let locked_version: i32 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if locked_version > supported {
            return Err(DbError::NewerSchema {
                found: locked_version,
                supported,
            });
        }
        if locked_version < supported {
            migrate(&tx, locked_version)?;
            tx.pragma_update(None, "user_version", supported)?;
        }
        tx.commit()?;
    }
    Ok(conn)
}

// Changing the journal mode can return BUSY without invoking SQLite's busy
// handler when two fresh openers upgrade their locks at once (M0-c / M2-a).
// Retry that idempotent initialization within the same five-second contention
// budget, verify the resulting mode is WAL, and restore the full busy_timeout.
fn enable_wal(conn: &Connection) -> Result<(), DbError> {
    use std::time::{Duration, Instant};
    let budget = Duration::from_secs(5);
    let started = Instant::now();
    loop {
        conn.busy_timeout(budget.saturating_sub(started.elapsed()))?;
        match conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0)) {
            Ok(mode) if mode.eq_ignore_ascii_case("wal") => break,
            Ok(_) => return Err(DbError::Message("could not enable WAL".into())),
            Err(error)
                if matches!(
                    error.sqlite_error_code(),
                    Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
                ) && started.elapsed() < budget =>
            {
                thread::sleep(
                    Duration::from_millis(10).min(budget.saturating_sub(started.elapsed())),
                );
            }
            Err(error) => return Err(error.into()),
        }
    }
    conn.busy_timeout(budget)?;
    Ok(())
}
