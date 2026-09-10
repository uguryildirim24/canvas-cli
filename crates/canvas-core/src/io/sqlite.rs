//! Download database bridge to the process-wide store worker.
//! Synchronous callers must be on the blocking pool and retain the identity lock.
use rusqlite::Connection;
use std::path::Path;
use std::time::Duration;

pub(crate) fn call<T: Send + 'static>(
    path: &Path,
    f: impl FnOnce(&mut Connection) -> Result<T, rusqlite::Error> + Send + 'static,
) -> Result<T, rusqlite::Error> {
    let path = path.to_owned();
    crate::store::auxiliary_sqlite(move || {
        // Close before returning to the caller: no cached manifest handle may
        // outlive the identity lock retained by the download session.
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        f(&mut connection)
    })
}
