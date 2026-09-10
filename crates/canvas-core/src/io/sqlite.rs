//! Process-wide `SQLite` worker. Synchronous callers must be on the blocking pool.
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, mpsc};

type Connections = HashMap<PathBuf, Connection>;
type Job = Box<dyn FnOnce(&mut Connections) + Send>;

pub(crate) fn call<T: Send + 'static>(
    path: &Path,
    f: impl FnOnce(&mut Connection) -> Result<T, rusqlite::Error> + Send + 'static,
) -> Result<T, rusqlite::Error> {
    static WORKER: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();
    let worker = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Job>(32);
        std::thread::Builder::new()
            .name("canvas-sqlite".into())
            .spawn(move || {
                let mut connections = Connections::new();
                while let Ok(job) = rx.recv() {
                    job(&mut connections);
                }
            })
            .expect("start SQLite worker");
        tx
    });
    let path = path.to_owned();
    let (tx, rx) = mpsc::sync_channel(1);
    worker
        .send(Box::new(move |connections| {
            let result = (|| {
                if !connections.contains_key(&path) {
                    connections.insert(path.clone(), Connection::open(&path)?);
                }
                f(connections.get_mut(&path).expect("opened connection"))
            })();
            let _ = tx.send(result);
        }))
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    rx.recv().map_err(|_| rusqlite::Error::InvalidQuery)?
}
