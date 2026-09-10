//! Dedicated `SQLite` thread and async `call` handle.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};

use rusqlite::{Connection, OpenFlags};
use thiserror::Error;

use crate::identity::{IdentityDocument, Paths};

use super::migrate::{self, CACHE_USER_VERSION, STATE_USER_VERSION};

const CHANNEL_BOUND: usize = 64;

type Job = Box<dyn FnOnce(&mut StoreConns) + Send>;

/// Open cache and state connections owned by the `SQLite` thread.
pub struct StoreConns {
    /// Disposable cache database.
    pub cache: Connection,
    /// Durable state database.
    pub state: Connection,
}

/// Handle to the process-wide `SQLite` worker.
pub struct Store {
    tx: SyncSender<JobMsg>,
    _join: JoinHandle<()>,
}

enum JobMsg {
    Work(Job),
    Shutdown,
}

/// Database open / call errors.
#[derive(Debug, Error)]
pub enum DbError {
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

impl Store {
    /// Open `cache.sqlite` and `state.sqlite` on a dedicated thread.
    pub fn open(paths: &Paths, _identity: &IdentityDocument) -> Result<Self, DbError> {
        std::fs::create_dir_all(&paths.identity_dir)
            .map_err(|e| DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;
        let cache_path = paths.cache_db.clone();
        let state_path = paths.state_db.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), DbError>>(1);
        let (tx, rx) = mpsc::sync_channel::<JobMsg>(CHANNEL_BOUND);
        let join = thread::Builder::new()
            .name("canvas-sqlite".into())
            .spawn(move || sqlite_thread(cache_path, state_path, ready_tx, rx))
            .map_err(|e| DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;
        ready_rx.recv().map_err(|_| DbError::WorkerClosed)??;
        Ok(Self { tx, _join: join })
    }

    /// Run `f` on the `SQLite` thread and return its result.
    pub async fn call<F, T>(&self, f: F) -> Result<T, DbError>
    where
        F: FnOnce(&mut StoreConns) -> Result<T, DbError> + Send + 'static,
        T: Send + 'static,
    {
        let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
        let job: Job = Box::new(move |conns| {
            let result = f(conns);
            let _ = resp_tx.send(result);
        });
        self.tx
            .send(JobMsg::Work(job))
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
        let job: Job = Box::new(move |conns| {
            let result = f(conns);
            let _ = resp_tx.send(result);
        });
        self.tx
            .send(JobMsg::Work(job))
            .map_err(|_| DbError::WorkerClosed)?;
        resp_rx.recv().map_err(|_| DbError::WorkerPanicked)?
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = self.tx.send(JobMsg::Shutdown);
    }
}

#[allow(clippy::needless_pass_by_value)] // worker thread owns the channel ends
fn sqlite_thread(
    cache_path: impl AsRef<Path>,
    state_path: impl AsRef<Path>,
    ready: SyncSender<Result<(), DbError>>,
    rx: Receiver<JobMsg>,
) {
    let result = (|| -> Result<StoreConns, DbError> {
        let cache = open_db(
            cache_path.as_ref(),
            CACHE_USER_VERSION,
            migrate::migrate_cache,
        )?;
        let state = open_db(
            state_path.as_ref(),
            STATE_USER_VERSION,
            migrate::migrate_state,
        )?;
        Ok(StoreConns { cache, state })
    })();
    let mut conns = match result {
        Ok(c) => {
            let _ = ready.send(Ok(()));
            c
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    while let Ok(msg) = rx.recv() {
        match msg {
            JobMsg::Work(job) => job(&mut conns),
            JobMsg::Shutdown => break,
        }
    }
}

fn open_db(
    path: &Path,
    supported: i32,
    migrate: fn(&Connection) -> Result<(), DbError>,
) -> Result<Connection, DbError> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;",
    )?;
    let found: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if found > supported {
        return Err(DbError::NewerSchema { found, supported });
    }
    if found < supported {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        match migrate(&conn) {
            Ok(()) => {
                conn.execute(&format!("PRAGMA user_version = {supported}"), [])?;
                conn.execute_batch("COMMIT")?;
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
    }
    Ok(conn)
}
