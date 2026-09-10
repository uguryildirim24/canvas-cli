//! Journal create, transitions, and recovery.

use std::path::Path;
use std::str::FromStr;

use jiff::Timestamp;
use rusqlite::{OptionalExtension, params};
use thiserror::Error;
use uuid::Uuid;

use crate::journal::locks::{LockError, OwnerLock, probe_owner};
use crate::journal::record::{PostedRecord, ReceiptRecord};
use crate::journal::state::{NotSubmittedEvidence, OwnerStatus, ResponseKind, State};
use crate::store::{DbError, Store, bump_epochs};

/// Journal domain errors.
#[derive(Debug, Error)]
pub enum JournalError {
    /// Another submit holds the admission lock or unique index.
    #[error("in_progress")]
    InProgress,
    /// Expected-state guard matched zero rows.
    #[error("state conflict")]
    StateConflict,
    /// Journal row missing.
    #[error("not found")]
    NotFound,
    /// Lock error.
    #[error(transparent)]
    Lock(#[from] LockError),
    /// Store / SQLite error.
    #[error(transparent)]
    Store(#[from] DbError),
    /// I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON error.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl From<rusqlite::Error> for JournalError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Store(DbError::from(value))
    }
}

/// Inputs for `create` (caller holds admission lock).
#[derive(Debug, Clone)]
pub struct CreateOpts {
    /// Identity key.
    pub identity_key: String,
    /// Course id.
    pub course_id: i64,
    /// Assignment id.
    pub assignment_id: i64,
    /// Submission kind.
    pub kind: String,
    /// Frozen intended payload JSON.
    pub intended_payload_json: String,
    /// Baseline attempt.
    pub baseline_attempt: Option<i64>,
    /// Baseline submission id.
    pub baseline_submission_id: Option<i64>,
}

/// Optional column patches for a transition.
#[derive(Debug, Clone, Default)]
pub struct TransitionPatch {
    /// Error text.
    pub error_text: Option<String>,
    /// HTTP post status.
    pub post_status: Option<Option<i64>>,
    /// Response kind.
    pub response_kind: Option<ResponseKind>,
    /// Not-submitted evidence.
    pub not_submitted_evidence: Option<NotSubmittedEvidence>,
    /// Allowlisted response JSON.
    pub response_record_json: Option<String>,
    /// Readback JSON.
    pub readback_record_json: Option<String>,
    /// Server match JSON.
    pub server_match_json: Option<String>,
    /// Receipt JSON.
    pub receipt_record_json: Option<String>,
    /// Uploaded file ids JSON array.
    pub uploaded_file_ids_json: Option<String>,
    /// Set `posting_started_at` to now when true.
    pub set_posting_started_at: bool,
}

/// Loaded journal row.
#[derive(Debug, Clone)]
pub struct JournalRow {
    /// Journal id.
    pub journal_id: String,
    /// State.
    pub state: State,
    /// Assignment id.
    pub assignment_id: i64,
    /// Course id.
    pub course_id: i64,
    /// Kind.
    pub kind: String,
    /// Uploaded file ids JSON.
    pub uploaded_file_ids_json: String,
    /// Response record JSON.
    pub response_record_json: Option<String>,
    /// Receipt record JSON.
    pub receipt_record_json: Option<String>,
    /// Acknowledged at.
    pub acknowledged_at: Option<String>,
    /// Error text.
    pub error_text: Option<String>,
}

/// Create a `planned` journal: owner lock before insert; unique index last.
///
/// Caller must hold [`AdmissionLock`] and release it after this returns.
pub fn create(
    store: &Store,
    identity_dir: &Path,
    opts: &CreateOpts,
) -> Result<(String, OwnerLock), JournalError> {
    let journal_id = Uuid::new_v4().to_string();
    let owner = OwnerLock::acquire(identity_dir, &journal_id)?;
    let now = Timestamp::now().to_string();
    let identity_key = opts.identity_key.clone();
    let course_id = opts.course_id;
    let assignment_id = opts.assignment_id;
    let kind = opts.kind.clone();
    let payload = opts.intended_payload_json.clone();
    let baseline_attempt = opts.baseline_attempt;
    let baseline_submission_id = opts.baseline_submission_id;
    let jid = journal_id.clone();

    let result = store.call_blocking(move |conns| {
        let tx = conns.state.unchecked_transaction()?;
        let insert = tx.execute(
            "INSERT INTO submission_journal (
                journal_id, identity_key, course_id, assignment_id, kind, state,
                intended_payload_json, baseline_attempt, baseline_submission_id,
                created_at, planned_at
             ) VALUES (?1,?2,?3,?4,?5,'planned',?6,?7,?8,?9,?9)",
            params![
                jid,
                identity_key,
                course_id,
                assignment_id,
                kind,
                payload,
                baseline_attempt,
                baseline_submission_id,
                now,
            ],
        );
        match insert {
            Ok(_) => {
                tx.commit()?;
                Ok(())
            }
            Err(rusqlite::Error::SqliteFailure(code, _))
                if code.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(DbError::from(rusqlite::Error::SqliteFailure(
                    code,
                    Some("in_progress".into()),
                )))
            }
            Err(e) => Err(DbError::from(e)),
        }
    });

    match result {
        Ok(()) => Ok((journal_id, owner)),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("UNIQUE") || msg.contains("in_progress") || msg.contains("constraint") {
                Err(JournalError::InProgress)
            } else {
                Err(JournalError::Store(e))
            }
        }
    }
}

/// Guarded state transition.
pub fn transition(
    store: &Store,
    journal_id: &str,
    from: State,
    to: State,
    patch: TransitionPatch,
) -> Result<(), JournalError> {
    let jid = journal_id.to_string();
    let from_s = from.as_str().to_string();
    let to_s = to.as_str().to_string();
    let now = Timestamp::now().to_string();
    let ts_col = match to {
        State::Planned => "planned_at",
        State::Uploading => "uploading_at",
        State::Uploaded => "uploaded_at",
        State::Posting => "posting_at",
        State::Submitted => "submitted_at",
        State::Matched => "matched_at",
        State::UploadIncomplete => "upload_incomplete_at",
        State::UploadedNotSubmitted => "uploaded_not_submitted_at",
        State::OutcomeUnknown => "outcome_unknown_at",
        State::Refused => "refused_at",
    };
    let terminal = to.is_terminal();

    store.call_blocking(move |conns| {
        let tx = conns.state.unchecked_transaction()?;
        let mut sql = format!(
            "UPDATE submission_journal SET state = ?1, {ts_col} = ?2"
        );
        if terminal {
            sql.push_str(", terminal_at = ?2");
        }
        if patch.set_posting_started_at {
            sql.push_str(", posting_started_at = ?2");
        }
        if patch.error_text.is_some() {
            sql.push_str(", error_text = ?3");
        }
        if patch.response_kind.is_some() {
            sql.push_str(", response_kind = ?4");
        }
        if patch.not_submitted_evidence.is_some() {
            sql.push_str(", not_submitted_evidence = ?5");
        }
        if patch.response_record_json.is_some() {
            sql.push_str(", response_record_json = ?6");
        }
        if patch.receipt_record_json.is_some() {
            sql.push_str(", receipt_record_json = ?7");
        }
        if patch.readback_record_json.is_some() {
            sql.push_str(", readback_record_json = ?8");
        }
        if patch.server_match_json.is_some() {
            sql.push_str(", server_match_json = ?9");
        }
        if patch.uploaded_file_ids_json.is_some() {
            sql.push_str(", uploaded_file_ids_json = ?10");
        }
        if let Some(Some(_)) = patch.post_status {
            sql.push_str(", post_status = ?11");
        } else if let Some(None) = patch.post_status {
            sql.push_str(", post_status = NULL");
        }
        sql.push_str(" WHERE journal_id = ?12 AND state = ?13");

        let changed = tx.execute(
            &sql,
            params![
                to_s,
                now,
                patch.error_text,
                patch.response_kind.map(|k| k.as_str().to_string()),
                patch
                    .not_submitted_evidence
                    .map(|e| e.as_str().to_string()),
                patch.response_record_json,
                patch.receipt_record_json,
                patch.readback_record_json,
                patch.server_match_json,
                patch.uploaded_file_ids_json,
                patch.post_status.and_then(|p| p),
                jid,
                from_s,
            ],
        )?;
        if changed == 0 {
            return Err(DbError::Message("state conflict".into()));
        }
        tx.commit()?;
        Ok(())
    })
    .map_err(|e| {
        if e.to_string().contains("state conflict") {
            JournalError::StateConflict
        } else {
            JournalError::Store(e)
        }
    })
}

/// Append a Canvas file id after a successful upload.
pub fn append_uploaded_file_id(
    store: &Store,
    journal_id: &str,
    file_id: i64,
) -> Result<(), JournalError> {
    let jid = journal_id.to_string();
    store.call_blocking(move |conns| {
        let tx = conns.state.unchecked_transaction()?;
        let current: String = tx.query_row(
            "SELECT uploaded_file_ids_json FROM submission_journal WHERE journal_id = ?1",
            params![jid],
            |r| r.get(0),
        )?;
        let mut ids: Vec<i64> = serde_json::from_str(&current).unwrap_or_default();
        ids.push(file_id);
        let next = serde_json::to_string(&ids).map_err(|e| DbError::Message(e.to_string()))?;
        tx.execute(
            "UPDATE submission_journal SET uploaded_file_ids_json = ?1 WHERE journal_id = ?2",
            params![next, jid],
        )?;
        tx.commit()?;
        Ok(())
    })?;
    Ok(())
}

/// Transition `uploaded` → `posting` and set `posting_started_at`.
pub fn mark_posting(store: &Store, journal_id: &str) -> Result<(), JournalError> {
    transition(
        store,
        journal_id,
        State::Uploaded,
        State::Posting,
        TransitionPatch {
            set_posting_started_at: true,
            ..TransitionPatch::default()
        },
    )
}

/// Success transaction: allowlisted record + receipt + `submitted` + epoch bump.
pub fn commit_success(
    store: &Store,
    journal_id: &str,
    course_id: i64,
    assignment_id: i64,
    posted: &PostedRecord,
    receipt: &ReceiptRecord,
) -> Result<(), JournalError> {
    let jid = journal_id.to_string();
    let response_json = serde_json::to_string(posted)?;
    let receipt_json = serde_json::to_string(receipt)?;
    let now = Timestamp::now().to_string();
    let ag = format!("assignment_groups:course:{course_id}:");
    let submission_scope = format!("submission:assignment:{assignment_id}");
    let assignments_scope = format!("assignments:course:{course_id}");
    let totals_scope = format!("course_totals:course:{course_id}");

    store.call_blocking(move |conns| {
        let tx = conns.state.unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE submission_journal SET
                state = 'submitted',
                submitted_at = ?1,
                terminal_at = ?1,
                response_record_json = ?2,
                receipt_record_json = ?3
             WHERE journal_id = ?4 AND state = 'posting'",
            params![now, response_json, receipt_json, jid],
        )?;
        if changed == 0 {
            return Err(DbError::Message("state conflict".into()));
        }
        bump_epochs(
            &[
                submission_scope.as_str(),
                assignments_scope.as_str(),
                ag.as_str(),
                "missing:all",
                "planner:*",
                "enrollment_grades:*",
                totals_scope.as_str(),
            ],
            &tx,
        )?;
        tx.commit()?;
        Ok(())
    })
    .map_err(|e| {
        if e.to_string().contains("state conflict") {
            JournalError::StateConflict
        } else {
            JournalError::Store(e)
        }
    })
}

/// Owner-absent recovery. `Ok(None)` if owner is live.
pub fn recover_if_owner_absent(
    store: &Store,
    identity_dir: &Path,
    journal_id: &str,
) -> Result<Option<State>, JournalError> {
    let Some(_lock) = OwnerLock::try_acquire(identity_dir, journal_id)? else {
        return Ok(None);
    };
    let row = get_journal(store, journal_id)?.ok_or(JournalError::NotFound)?;
    let (to, patch) = match row.state {
        State::Planned => (
            State::Refused,
            TransitionPatch {
                error_text: Some("abandoned before upload".into()),
                ..TransitionPatch::default()
            },
        ),
        State::Uploading => (State::UploadIncomplete, TransitionPatch::default()),
        State::Uploaded => (
            State::UploadedNotSubmitted,
            TransitionPatch {
                not_submitted_evidence: Some(NotSubmittedEvidence::NeverSent),
                ..TransitionPatch::default()
            },
        ),
        State::Posting => (
            State::OutcomeUnknown,
            TransitionPatch {
                response_kind: Some(ResponseKind::None),
                ..TransitionPatch::default()
            },
        ),
        other => return Ok(Some(other)),
    };
    transition(store, journal_id, row.state, to, patch)?;
    Ok(Some(to))
}

/// Load a journal row.
pub fn get_journal(store: &Store, journal_id: &str) -> Result<Option<JournalRow>, JournalError> {
    let jid = journal_id.to_string();
    let row = store.call_blocking(move |conns| {
        conns
            .state
            .query_row(
                "SELECT journal_id, state, assignment_id, course_id, kind,
                        uploaded_file_ids_json, response_record_json, receipt_record_json,
                        acknowledged_at, error_text
                 FROM submission_journal WHERE journal_id = ?1",
                params![jid],
                |r| {
                    Ok(JournalRow {
                        journal_id: r.get(0)?,
                        state: State::from_str(&r.get::<_, String>(1)?).unwrap_or(State::Refused),
                        assignment_id: r.get(2)?,
                        course_id: r.get(3)?,
                        kind: r.get(4)?,
                        uploaded_file_ids_json: r.get(5)?,
                        response_record_json: r.get(6)?,
                        receipt_record_json: r.get(7)?,
                        acknowledged_at: r.get(8)?,
                        error_text: r.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(DbError::from)
    })?;
    Ok(row)
}

/// Acknowledge an `outcome_unknown` journal (pending hook).
pub fn acknowledge(store: &Store, journal_id: &str) -> Result<(), JournalError> {
    let jid = journal_id.to_string();
    let now = Timestamp::now().to_string();
    store.call_blocking(move |conns| {
        let tx = conns.state.unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE submission_journal SET acknowledged_at = ?1
             WHERE journal_id = ?2 AND state = 'outcome_unknown'",
            params![now, jid],
        )?;
        if changed == 0 {
            return Err(DbError::Message("state conflict".into()));
        }
        tx.commit()?;
        Ok(())
    })?;
    Ok(())
}

/// Owner status for JSON: `n/a` when terminal, else probe.
#[must_use]
pub fn owner_status_for(identity_dir: &Path, journal_id: &str, state: State) -> OwnerStatus {
    if state.is_terminal() {
        OwnerStatus::NotApplicable
    } else {
        probe_owner(identity_dir, journal_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{IdentityDocument, Paths};
    use crate::journal::locks::AdmissionLock;
    use crate::journal::record::{Evidence, allowlist_from_json};
    use crate::store::OpenIdentity;
    use serde_json::json;
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    fn setup() -> (TempDir, Paths, Store, IdentityDocument) {
        let dir = TempDir::new().unwrap();
        let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(dir.path(), &doc.key);
        std::fs::create_dir_all(&paths.identity_dir).unwrap();
        std::fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
        doc.write(&paths.identity_json()).unwrap();
        let open = OpenIdentity::open(&paths, &doc).unwrap();
        (dir, paths, open.store, doc)
    }

    fn opts_for(doc: &IdentityDocument, assignment: i64) -> CreateOpts {
        CreateOpts {
            identity_key: doc.key.to_string(),
            course_id: 1,
            assignment_id: assignment,
            kind: "online_upload".into(),
            intended_payload_json: "{}".into(),
            baseline_attempt: Some(0),
            baseline_submission_id: None,
        }
    }

    #[test]
    fn create_and_recover_planned() {
        let (_dir, paths, store, doc) = setup();
        let _adm = AdmissionLock::try_acquire(&paths.identity_dir, 42).unwrap();
        let (jid, owner) = create(&store, &paths.identity_dir, &opts_for(&doc, 42)).unwrap();
        drop(owner);
        let recovered = recover_if_owner_absent(&store, &paths.identity_dir, &jid)
            .unwrap()
            .unwrap();
        assert_eq!(recovered, State::Refused);
        let row = get_journal(&store, &jid).unwrap().unwrap();
        assert_eq!(row.state, State::Refused);
        assert_eq!(row.error_text.as_deref(), Some("abandoned before upload"));
    }

    #[test]
    fn recovery_table_for_active_states() {
        let (_dir, paths, store, doc) = setup();
        for (from, expect, assignment) in [
            (State::Uploading, State::UploadIncomplete, 101_i64),
            (State::Uploaded, State::UploadedNotSubmitted, 102),
            (State::Posting, State::OutcomeUnknown, 103),
        ] {
            let _adm = AdmissionLock::try_acquire(&paths.identity_dir, assignment).unwrap();
            let (jid, owner) =
                create(&store, &paths.identity_dir, &opts_for(&doc, assignment)).unwrap();
            transition(
                &store,
                &jid,
                State::Planned,
                from,
                if from == State::Posting {
                    TransitionPatch {
                        set_posting_started_at: true,
                        ..TransitionPatch::default()
                    }
                } else {
                    TransitionPatch::default()
                },
            )
            .unwrap();
            drop(owner);
            let got = recover_if_owner_absent(&store, &paths.identity_dir, &jid)
                .unwrap()
                .unwrap();
            assert_eq!(got, expect);
        }
    }

    #[test]
    fn live_owner_blocks_recovery() {
        let (_dir, paths, store, doc) = setup();
        let _adm = AdmissionLock::try_acquire(&paths.identity_dir, 7).unwrap();
        let (jid, _owner) = create(&store, &paths.identity_dir, &opts_for(&doc, 7)).unwrap();
        assert!(
            recover_if_owner_absent(&store, &paths.identity_dir, &jid)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            owner_status_for(&paths.identity_dir, &jid, State::Planned),
            OwnerStatus::Live
        );
    }

    #[test]
    fn two_simultaneous_creates_conflict() {
        let (_dir, paths, store, doc) = setup();
        let barrier = Arc::new(Barrier::new(2));
        let store = Arc::new(store);
        let idir = paths.identity_dir.clone();
        let key = doc.key.to_string();
        let mut handles = Vec::new();
        for _ in 0..2 {
            let store = Arc::clone(&store);
            let idir = idir.clone();
            let barrier = Arc::clone(&barrier);
            let key = key.clone();
            handles.push(thread::spawn(move || {
                let adm = AdmissionLock::try_acquire(&idir, 99);
                barrier.wait();
                let opts = CreateOpts {
                    identity_key: key,
                    course_id: 1,
                    assignment_id: 99,
                    kind: "online_upload".into(),
                    intended_payload_json: "{}".into(),
                    baseline_attempt: Some(0),
                    baseline_submission_id: None,
                };
                match adm {
                    Ok(_adm) => create(&store, &idir, &opts).map(|_| true),
                    Err(LockError::InProgress) => Ok(false),
                    Err(e) => Err(JournalError::from(e)),
                }
            }));
        }
        let results: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect();
        let successes = results.iter().filter(|r| matches!(r, Ok(true))).count();
        let blocked = results
            .iter()
            .filter(|r| matches!(r, Ok(false) | Err(JournalError::InProgress)))
            .count();
        assert_eq!(successes, 1);
        assert_eq!(blocked, 1);
    }

    #[test]
    fn two_recoverers_serialize() {
        let (_dir, paths, store, doc) = setup();
        let _adm = AdmissionLock::try_acquire(&paths.identity_dir, 11).unwrap();
        let (jid, owner) = create(&store, &paths.identity_dir, &opts_for(&doc, 11)).unwrap();
        drop(owner);
        let store = Arc::new(store);
        let idir = paths.identity_dir.clone();
        let jid2 = jid.clone();
        let a = {
            let store = Arc::clone(&store);
            let idir = idir.clone();
            let jid = jid.clone();
            thread::spawn(move || recover_if_owner_absent(&store, &idir, &jid))
        };
        let b = {
            let store = Arc::clone(&store);
            thread::spawn(move || recover_if_owner_absent(&store, &idir, &jid2))
        };
        let ra = a.join().unwrap().unwrap();
        let rb = b.join().unwrap().unwrap();
        assert!(ra == Some(State::Refused) || rb == Some(State::Refused));
        let row = get_journal(&store, &jid).unwrap().unwrap();
        assert_eq!(row.state, State::Refused);
    }

    #[test]
    fn commit_success_atomic_with_receipt() {
        let (_dir, paths, store, doc) = setup();
        let _adm = AdmissionLock::try_acquire(&paths.identity_dir, 3).unwrap();
        let (jid, _owner) = create(&store, &paths.identity_dir, &opts_for(&doc, 3)).unwrap();
        transition(&store, &jid, State::Planned, State::Uploading, TransitionPatch::default())
            .unwrap();
        transition(&store, &jid, State::Uploading, State::Uploaded, TransitionPatch::default())
            .unwrap();
        mark_posting(&store, &jid).unwrap();
        let posted = allowlist_from_json(
            Evidence::PostResponse,
            &json!({"id": 5, "attempt": 1}),
            Some(br#"{"id":5,"attempt":1}"#),
        )
        .unwrap();
        let receipt = ReceiptRecord {
            receipt_id: "r1".into(),
            journal_id: jid.clone(),
            attribution: "observed".into(),
            posted: posted.clone(),
            readback: None,
        };
        commit_success(&store, &jid, 1, 3, &posted, &receipt).unwrap();
        let row = get_journal(&store, &jid).unwrap().unwrap();
        assert_eq!(row.state, State::Submitted);
        assert!(row.receipt_record_json.is_some());
        assert!(row.response_record_json.is_some());
    }

    #[test]
    fn planned_kill_helper_subprocess() {
        if let Ok(root) = std::env::var("CANVAS_JOURNAL_KILL_ROOT") {
            let key = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z").key;
            let paths = Paths::for_identity(root, &key);
            let jid = std::env::var("CANVAS_JOURNAL_KILL_ID").unwrap();
            let _owner = OwnerLock::acquire(&paths.identity_dir, &jid).unwrap();
            std::fs::write(paths.data_root.join("ready"), "1").unwrap();
            thread::sleep(Duration::from_secs(2));
            return;
        }

        let (dir, paths, store, doc) = setup();
        let jid = Uuid::new_v4().to_string();
        let exe = std::env::current_exe().unwrap();
        let mut child = Command::new(&exe)
            .args([
                "--exact",
                "journal::ops::tests::planned_kill_helper_subprocess",
            ])
            .env("CANVAS_JOURNAL_KILL_ROOT", dir.path())
            .env("CANVAS_JOURNAL_KILL_ID", &jid)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let ready = dir.path().join("ready");
        for _ in 0..200 {
            if ready.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(ready.exists());
        // While child holds owner lock, create with same id is not used; probe is live.
        assert_eq!(probe_owner(&paths.identity_dir, &jid), OwnerStatus::Live);
        let _ = child.kill();
        let _ = child.wait();
        // After kill, recovery path for a real journal still works (separate id).
        let _adm = AdmissionLock::try_acquire(&paths.identity_dir, 55).unwrap();
        let (real, owner) = create(&store, &paths.identity_dir, &opts_for(&doc, 55)).unwrap();
        drop(owner);
        assert_eq!(
            recover_if_owner_absent(&store, &paths.identity_dir, &real)
                .unwrap()
                .unwrap(),
            State::Refused
        );
    }
}
