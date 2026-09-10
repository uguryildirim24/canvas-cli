//! Foreground submission interest (REPORT §3.6).
//!
//! A `submit` or `plan execute` registers before its first pre-flight request.
//! While a live registration exists, `watch` admits no new polling request and
//! holds no slot, so the foreground wait is bounded by the requests already in
//! flight — one, at `api_concurrency = 1`.
//!
//! The row lives in `state.sqlite`; liveness is the matching
//! `locks/interest-assignment-<id>.lock`. A registrant that dies frees its
//! interest with its descriptor, so polling can never be starved by a crash.
//! One lock file per assignment keeps the set bounded, like the §12.2
//! admission locks.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fs4::fs_std::FileExt;
use jiff::Timestamp;
use rusqlite::{TransactionBehavior, params};

use super::{CoordError, open_lock_file};

/// What kind of foreground work registered the interest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterestKind {
    /// The human `submit` command.
    Submit,
    /// `plan execute` (M6-a).
    PlanExecute,
}

impl InterestKind {
    /// The value stored in `interest.kind`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Submit => "submit",
            Self::PlanExecute => "plan_execute",
        }
    }
}

/// A live registration. Dropping it removes the row and releases the lock.
pub struct Interest {
    conn: Arc<Mutex<rusqlite::Connection>>,
    file: File,
    assignment_id: i64,
}

impl std::fmt::Debug for Interest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Interest")
            .field("assignment_id", &self.assignment_id)
            .finish_non_exhaustive()
    }
}

impl Interest {
    /// The assignment this interest is registered for.
    #[must_use]
    pub fn assignment_id(&self) -> i64 {
        self.assignment_id
    }
}

impl Drop for Interest {
    fn drop(&mut self) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "DELETE FROM interest WHERE assignment_id = ?1",
                params![self.assignment_id],
            );
        }
        let _ = FileExt::unlock(&self.file);
    }
}

fn lock_path(locks_dir: &Path, assignment_id: i64) -> PathBuf {
    locks_dir.join(format!("interest-assignment-{assignment_id}.lock"))
}

/// Register interest for one assignment.
pub(super) fn register(
    locks_dir: &Path,
    conn: Arc<Mutex<rusqlite::Connection>>,
    kind: InterestKind,
    assignment_id: i64,
) -> Result<Option<Interest>, CoordError> {
    let file = open_lock_file(&lock_path(locks_dir, assignment_id))?;
    if !FileExt::try_lock_exclusive(&file)? {
        return Ok(None);
    }
    {
        let mut guard = conn.lock().expect("coordinator connection");
        let tx = guard.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO interest (assignment_id, kind, registered_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(assignment_id) DO UPDATE SET
                kind = excluded.kind, registered_at = excluded.registered_at",
            params![assignment_id, kind.as_str(), Timestamp::now().to_string()],
        )?;
        tx.commit()?;
    }
    Ok(Some(Interest {
        conn,
        file,
        assignment_id,
    }))
}

/// Whether any live interest is registered, removing the dead rows.
pub(super) fn any_live(
    locks_dir: &Path,
    conn: &Arc<Mutex<rusqlite::Connection>>,
) -> Result<bool, CoordError> {
    let rows: Vec<i64> = {
        let guard = conn.lock().expect("coordinator connection");
        let mut stmt = guard.prepare("SELECT assignment_id FROM interest")?;
        let ids = stmt.query_map([], |r| r.get(0))?;
        ids.collect::<Result<Vec<_>, _>>()?
    };
    let mut live = false;
    for assignment_id in rows {
        let file = open_lock_file(&lock_path(locks_dir, assignment_id))?;
        if FileExt::try_lock_exclusive(&file)? {
            // Nobody holds it: the registrant is gone, so the row is stale.
            let _ = FileExt::unlock(&file);
            let guard = conn.lock().expect("coordinator connection");
            guard.execute(
                "DELETE FROM interest WHERE assignment_id = ?1",
                params![assignment_id],
            )?;
        } else {
            live = true;
        }
    }
    Ok(live)
}
