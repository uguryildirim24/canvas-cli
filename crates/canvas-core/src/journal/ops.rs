//! Journal create, transitions, and recovery.

use std::path::Path;
use std::str::FromStr;

use crate::journal::locks::AdmissionLock;
use jiff::Timestamp;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
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
    /// Store / `SQLite` error.
    #[error(transparent)]
    Store(DbError),
    /// I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON error.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl From<DbError> for JournalError {
    fn from(error: DbError) -> Self {
        if matches!(&error, DbError::Message(s) if s == "state conflict") {
            Self::StateConflict
        } else {
            Self::Store(error)
        }
    }
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
    /// Frozen [`super::IntendedPayload`] JSON; unknown fields are rejected.
    pub intended_payload_json: String,
    /// Baseline attempt.
    pub baseline_attempt: Option<i64>,
    /// Baseline submission id.
    pub baseline_submission_id: Option<i64>,
}

/// The approved plan a journal is created for (REPORT §3.5).
///
/// Linking happens inside the journal insert transaction, so a journal and its
/// plan link are published together or not at all. The unique index on
/// `submission_journal.plan_id` and the `state = 'approved'` guard on the plan
/// row both refuse a second journal for one plan.
#[derive(Debug, Clone)]
pub struct PlanLink {
    /// Plan id.
    pub plan_id: String,
    /// The approval audit copied into the journal row.
    pub approval_json: String,
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
    /// Server match JSON.
    pub server_match_json: Option<String>,
    /// Compatibility hint; entering `posting` always writes `posting_started_at`.
    pub set_posting_started_at: bool,
}

/// Loaded journal row.
#[derive(Debug, Clone)]
pub struct JournalRow {
    pub identity_key: String,
    pub intended_payload_json: String,
    pub baseline_attempt: Option<i64>,
    pub baseline_submission_id: Option<i64>,
    pub created_at: String,
    pub planned_at: Option<String>,
    pub uploading_at: Option<String>,
    pub uploaded_at: Option<String>,
    pub posting_at: Option<String>,
    pub posting_started_at: Option<String>,
    pub submitted_at: Option<String>,
    pub matched_at: Option<String>,
    pub terminal_at: Option<String>,
    pub upload_incomplete_at: Option<String>,
    pub uploaded_not_submitted_at: Option<String>,
    pub outcome_unknown_at: Option<String>,
    pub refused_at: Option<String>,
    pub post_status: Option<i64>,
    pub response_kind: Option<String>,
    pub not_submitted_evidence: Option<String>,
    pub readback_record_json: Option<String>,
    pub server_match_json: Option<String>,
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
    /// The plan this journal was admitted from; `null` for legacy rows.
    pub plan_id: Option<String>,
    /// The approval audit copied in at admission; `null` for legacy rows.
    pub approval_json: Option<String>,
}

/// Create a `planned` journal: owner lock before insert; unique index last.
///
/// Caller must hold [`AdmissionLock`] and release it after this returns.
pub fn create(
    store: &Store,
    identity_dir: &Path,
    admission: &AdmissionLock,
    opts: &CreateOpts,
) -> Result<(String, OwnerLock), JournalError> {
    create_linked(store, identity_dir, admission, opts, None)
}

/// Create a `planned` journal and, in the same transaction, link an approved plan.
///
/// With `plan = None` this is [`create`]. With a link, the plan moves to
/// `executed` and its approval audit is copied into the journal row inside the
/// one transaction REPORT §3.5 requires; nothing uploads before it commits.
pub fn create_linked(
    store: &Store,
    identity_dir: &Path,
    admission: &AdmissionLock,
    opts: &CreateOpts,
    plan: Option<&PlanLink>,
) -> Result<(String, OwnerLock), JournalError> {
    if !admission.matches(identity_dir, opts.assignment_id) {
        return Err(JournalError::StateConflict);
    }
    verify_store_directory(store, identity_dir)?;
    let journal_id = Uuid::new_v4().to_string();
    let owner = OwnerLock::acquire(identity_dir, &journal_id)?;
    #[cfg(test)]
    super::crash_tests::checkpoint("owner_acquired", &journal_id);
    let now = Timestamp::now().to_string();
    let identity_key = opts.identity_key.clone();
    let course_id = opts.course_id;
    let assignment_id = opts.assignment_id;
    let kind = opts.kind.clone();
    let payload = serde_json::to_string(&serde_json::from_str::<super::IntendedPayload>(
        &opts.intended_payload_json,
    )?)?;
    let baseline_attempt = Some(opts.baseline_attempt.unwrap_or(0));
    let baseline_submission_id = opts.baseline_submission_id;
    let plan_id = plan.map(|p| p.plan_id.clone());
    let approval_json = plan.map(|p| p.approval_json.clone());
    let jid = journal_id.clone();

    let result = store.call_blocking(move |conns| {
        let tx = conns
            .state
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let key: String =
            tx.query_row("SELECT value FROM identity WHERE key = 'key'", [], |r| {
                r.get(0)
            })?;
        if key != identity_key {
            return Err(DbError::Message("identity mismatch".into()));
        }
        let insert = tx.execute(
            "INSERT INTO submission_journal (
                journal_id, identity_key, course_id, assignment_id, kind, state,
                intended_payload_json, baseline_attempt, baseline_submission_id,
                created_at, planned_at, plan_id, approval_json
             ) VALUES (?1,?2,?3,?4,?5,'planned',?6,?7,?8,?9,?9,?10,?11)",
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
                plan_id,
                approval_json,
            ],
        );
        match insert {
            Ok(_) => {
                // Consume the approval in the same transaction. The guard is the
                // plan's own state: a second execute finds it already `executed`.
                if let Some(plan_id) = &plan_id {
                    let linked = tx.execute(
                        "UPDATE plans SET state = 'executed', journal_id = ?1
                         WHERE plan_id = ?2 AND state = 'approved'",
                        params![jid, plan_id],
                    )?;
                    if linked != 1 {
                        return Err(DbError::Message("state conflict".into()));
                    }
                }
                crate::events::record_submission_state(&tx, &jid, None, "planned")?;
                #[cfg(test)]
                super::crash_tests::checkpoint("inserted", &jid);
                tx.commit()?;
                #[cfg(test)]
                super::crash_tests::checkpoint("published", &jid);
                Ok(())
            }
            Err(rusqlite::Error::SqliteFailure(code, _))
                if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
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
        Err(DbError::Sqlite(rusqlite::Error::SqliteFailure(_, Some(message))))
            if message == "in_progress" =>
        {
            Err(JournalError::InProgress)
        }
        Err(e) => Err(JournalError::from(e)),
    }
}

/// Guarded state transition; confirmed outcomes use the receipt helpers below.
pub fn transition(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    from: State,
    to: State,
    patch: TransitionPatch,
) -> Result<(), JournalError> {
    verify_store_directory(store, owner.identity_dir())?;
    if !owner.matches(journal_id)
        || !matches!(
            (from, to),
            (
                State::Planned,
                State::Uploading | State::Uploaded | State::Refused
            ) | (
                State::Uploading,
                State::Uploaded | State::UploadIncomplete | State::Refused
            ) | (
                State::Uploaded,
                State::Posting | State::UploadedNotSubmitted | State::Refused
            ) | (
                State::Posting | State::OutcomeUnknown,
                State::OutcomeUnknown
            )
        )
    {
        return Err(JournalError::StateConflict);
    }
    let mut patch = validate_patch(patch)?;
    if let Some(raw) = &mut patch.server_match_json {
        let row = get_journal(store, journal_id)?.ok_or(JournalError::NotFound)?;
        let intent: super::IntendedPayload = serde_json::from_str(&row.intended_payload_json)?;
        let mut candidate: Option<super::CandidateRecord> = serde_json::from_str(raw)?;
        if let Some(candidate) = &mut candidate {
            candidate.submitted_at_local =
                super::record::local_timestamp(candidate.submitted_at.as_deref(), &intent.zone()?)?;
        }
        *raw = serde_json::to_string(&candidate)?;
    }
    if to == State::UploadedNotSubmitted
        && patch.not_submitted_evidence != Some(NotSubmittedEvidence::NeverSent)
    {
        return Err(JournalError::StateConflict);
    }
    let jid = journal_id.to_owned();
    let now = Timestamp::now().to_string();
    let ts_col = format!("{}_at", to.as_str());
    let (from_state, to_state) = (from.as_str(), to.as_str());
    store
        .call_blocking(move |conns| {
            let tx = conns
                .state
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let sql = format!(
                "UPDATE submission_journal SET state = ?1, {ts_col} = ?2,
            terminal_at = CASE WHEN ?3 THEN ?2 ELSE terminal_at END,
            posting_started_at = CASE WHEN ?4 THEN ?2 ELSE posting_started_at END,
            error_text = COALESCE(?5, error_text),
            response_kind = COALESCE(?6, response_kind),
            not_submitted_evidence = COALESCE(?7, not_submitted_evidence),
            server_match_json = CASE WHEN ?8 = 'null' THEN NULL ELSE COALESCE(?8, server_match_json) END,
            post_status = CASE WHEN ?9 THEN ?10 ELSE post_status END
            WHERE journal_id = ?11 AND state = ?12"
            );
            let changed = tx.execute(
                &sql,
                params![
                    to.as_str(),
                    now,
                    to.is_terminal(),
                    to == State::Posting,
                    patch.error_text,
                    patch.response_kind.map(ResponseKind::as_str),
                    patch
                        .not_submitted_evidence
                        .map(NotSubmittedEvidence::as_str),
                    patch.server_match_json,
                    patch.post_status.is_some(),
                    patch.post_status.flatten(),
                    jid,
                    from.as_str()
                ],
            )?;
            if changed != 1 {
                return Err(DbError::Message("state conflict".into()));
            }
            bump_journal_epochs(&tx, &jid)?;
            // §12.2: the journal transition and its event are one transaction.
            crate::events::record_submission_state(&tx, &jid, Some(from_state), to_state)?;
            tx.commit()?;
            Ok(())
        })
        .map_err(JournalError::from)
}

/// Append a Canvas file id after a successful upload.
pub fn append_uploaded_file_id(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    file_index: usize,
    file_id: i64,
) -> Result<(), JournalError> {
    verify_store_directory(store, owner.identity_dir())?;
    if !owner.matches(journal_id) {
        return Err(JournalError::StateConflict);
    }
    let jid = journal_id.to_string();
    store.call_blocking(move |conns| {
        let tx = conns.state.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: String = tx.query_row(
            "SELECT uploaded_file_ids_json FROM submission_journal WHERE journal_id = ?1 AND state = 'uploading'",
            params![jid],
            |r| r.get(0),
        ).optional()?.ok_or_else(|| DbError::Message("state conflict".into()))?;
        let mut ids: Vec<i64> = serde_json::from_str(&current).map_err(|_| DbError::Message("invalid uploaded ids".into()))?;
        if file_id <= 0 || ids.contains(&file_id) { return Err(DbError::Message("invalid uploaded id".into())); }
        let payload: String = tx.query_row("SELECT intended_payload_json FROM submission_journal WHERE journal_id = ?1", [&jid], |r| r.get(0))?;
        let mut intent: super::IntendedPayload = serde_json::from_str(&payload).map_err(|_| DbError::Message("invalid intent".into()))?;
        let file = intent.files.get_mut(file_index).ok_or_else(|| DbError::Message("invalid file index".into()))?;
        if file.canvas_file_id.is_some() { return Err(DbError::Message("file already uploaded".into())); }
        file.canvas_file_id = Some(file_id.to_string());
        let payload = serde_json::to_string(&intent).map_err(|_| DbError::Message("invalid intent".into()))?;
        ids.push(file_id);
        let next = serde_json::to_string(&ids).map_err(|e| DbError::Message(e.to_string()))?;
        tx.execute(
            "UPDATE submission_journal SET uploaded_file_ids_json = ?1, intended_payload_json = ?3 WHERE journal_id = ?2 AND state = 'uploading'",
            params![next, jid, payload],
        )?;
        tx.commit()?;
        Ok(())
    })?;
    Ok(())
}

/// Transition `uploaded` → `posting` and set `posting_started_at`.
pub fn mark_posting(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
) -> Result<(), JournalError> {
    transition(
        store,
        owner,
        journal_id,
        State::Uploaded,
        State::Posting,
        TransitionPatch {
            set_posting_started_at: true,
            ..TransitionPatch::default()
        },
    )
}

/// Store observed success, its complete receipt, and all affected epochs atomically.
pub fn commit_success(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    post_status: u16,
    receipt: &ReceiptRecord,
) -> Result<(), JournalError> {
    if !(200..300).contains(&post_status) || receipt.readback.is_some() {
        return Err(JournalError::StateConflict);
    }
    commit_confirmed(
        store,
        owner,
        journal_id,
        State::Submitted,
        Some(post_status),
        receipt,
    )
}

/// Store a history-file match; attribution is explicitly unproven. Network selection is M2-b.
pub fn commit_matched(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    receipt: &ReceiptRecord,
) -> Result<(), JournalError> {
    commit_confirmed(store, owner, journal_id, State::Matched, None, receipt)
}

fn commit_confirmed(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    target: State,
    status: Option<u16>,
    receipt: &ReceiptRecord,
) -> Result<(), JournalError> {
    let row = get_journal(store, journal_id)?.ok_or(JournalError::NotFound)?;
    let mut receipt = receipt.clone();
    let intent: super::IntendedPayload = serde_json::from_str(&row.intended_payload_json)?;
    let zone = intent.zone()?;
    receipt.posted.submitted_at_local =
        super::record::local_timestamp(receipt.posted.submitted_at.as_deref(), &zone)?;
    if let Some(readback) = &mut receipt.readback {
        readback.submitted_at_local =
            super::record::local_timestamp(readback.submitted_at.as_deref(), &zone)?;
    }
    let posted = &receipt.posted;
    let observed = target == State::Submitted;
    verify_store_directory(store, owner.identity_dir())?;
    if !owner.matches(journal_id)
        || receipt.journal_id != journal_id
        || receipt.receipt_id.is_empty()
        || posted
            .attempt
            .is_none_or(|a| a <= row.baseline_attempt.unwrap_or(0))
        || (observed
            && (receipt.attribution != "observed"
                || posted.evidence != super::Evidence::PostResponse
                || posted.response_sha256.is_none()))
        || (!observed
            && (receipt.attribution != "unproven"
                || posted.evidence != super::Evidence::HistoryFiles
                || posted.response_sha256.is_some()))
    {
        return Err(JournalError::StateConflict);
    }
    if !observed {
        validate_history_match(&row, &receipt)?;
    }
    let jid = journal_id.to_owned();
    let response = serde_json::to_string(posted)?;
    let readback = if observed {
        receipt
            .readback
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?
    } else {
        Some(full_readback(&receipt.posted)?.to_string())
    };
    let now = Timestamp::now().to_string();
    let from_state = if observed {
        "posting"
    } else {
        "outcome_unknown"
    };
    let to_state = target.as_str();
    store
        .call_blocking(move |conns| {
            let tx = conns
                .state
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let receipt_json = build_receipt(&tx, &jid, &receipt)?;
            let sql = format!(
                "UPDATE submission_journal SET state = ?1, {}_at = ?2,
            terminal_at = ?2, response_record_json = ?3, receipt_record_json = ?4,
            readback_record_json = ?5, post_status = COALESCE(?6, post_status)
            WHERE journal_id = ?7 AND state = ?8",
                target.as_str()
            );
            let changed = tx.execute(
                &sql,
                params![
                    target.as_str(),
                    now,
                    response,
                    receipt_json,
                    readback,
                    status,
                    jid,
                    if observed {
                        "posting"
                    } else {
                        "outcome_unknown"
                    }
                ],
            )?;
            if changed != 1 {
                return Err(DbError::Message("state conflict".into()));
            }
            bump_journal_epochs(&tx, &jid)?;
            crate::events::record_submission_state(&tx, &jid, Some(from_state), to_state)?;
            #[cfg(test)]
            super::crash_tests::checkpoint("success_before_commit", &jid);
            tx.commit()?;
            #[cfg(test)]
            super::crash_tests::checkpoint("success_after_commit", &jid);
            Ok(())
        })
        .map_err(JournalError::from)
}

fn validate_history_match(row: &JournalRow, receipt: &ReceiptRecord) -> Result<(), JournalError> {
    let posted = &receipt.posted;
    let ids: Vec<i64> = serde_json::from_str(&row.uploaded_file_ids_json)?;
    let expected: std::collections::BTreeSet<_> = ids.iter().map(ToString::to_string).collect();
    let actual: std::collections::BTreeSet<_> =
        posted.attachments.iter().map(|a| a.id.clone()).collect();
    if row.kind != "online_upload"
        || expected.is_empty()
        || expected != actual
        || receipt.readback.is_none()
    {
        return Err(JournalError::StateConflict);
    }
    let started: Timestamp = row
        .posting_started_at
        .as_deref()
        .ok_or(JournalError::StateConflict)?
        .parse()
        .map_err(|_| JournalError::StateConflict)?;
    let submitted: Timestamp = posted
        .submitted_at
        .as_deref()
        .ok_or(JournalError::StateConflict)?
        .parse()
        .map_err(|_| JournalError::StateConflict)?;
    if submitted.as_nanosecond() < started.as_nanosecond() - 300_000_000_000 {
        return Err(JournalError::StateConflict);
    }
    let readback = receipt
        .readback
        .as_ref()
        .ok_or(JournalError::StateConflict)?;
    if readback.attachments != posted.attachments
        || readback.body_sha256 != posted.body_sha256
        || readback.submitted_at != posted.submitted_at
        || readback.late != posted.late
    {
        return Err(JournalError::StateConflict);
    }
    Ok(())
}

fn full_readback(record: &PostedRecord) -> Result<serde_json::Value, JournalError> {
    let mut value = serde_json::to_value(record)?;
    if let Some(map) = value.as_object_mut() {
        map.remove("evidence");
        map.remove("response_sha256");
    }
    Ok(value)
}

/// Enrich only the recorded attempt, updating both journal and receipt in one transaction.
pub fn enrich_readback(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    attempt: i64,
    readback: &super::ReadbackRecord,
) -> Result<(), JournalError> {
    enrich_readback_impl(store, owner, journal_id, attempt, readback, None)
}

/// Enrich the stored full allowlist while projecting only Readback fields into the receipt.
pub fn enrich_readback_full(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    record: &PostedRecord,
) -> Result<(), JournalError> {
    let readback = super::ReadbackRecord {
        submitted_at: record.submitted_at.clone(),
        submitted_at_local: record.submitted_at_local.clone(),
        late: record.late,
        attachments: record.attachments.clone(),
        body_sha256: record.body_sha256.clone(),
    };
    enrich_readback_impl(
        store,
        owner,
        journal_id,
        record.attempt.ok_or(JournalError::StateConflict)?,
        &readback,
        Some(record),
    )
}

fn enrich_readback_impl(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    attempt: i64,
    readback: &super::ReadbackRecord,
    full: Option<&PostedRecord>,
) -> Result<(), JournalError> {
    verify_store_directory(store, owner.identity_dir())?;
    if !owner.matches(journal_id) {
        return Err(JournalError::StateConflict);
    }
    let row = get_journal(store, journal_id)?.ok_or(JournalError::NotFound)?;
    if !matches!(row.state, State::Submitted | State::Matched) {
        return Err(JournalError::StateConflict);
    }
    let posted: PostedRecord = serde_json::from_str(
        row.response_record_json
            .as_deref()
            .ok_or(JournalError::StateConflict)?,
    )?;
    if posted.attempt != Some(attempt) {
        return Err(JournalError::StateConflict);
    }
    let mut readback = readback.clone();
    let intent: super::IntendedPayload = serde_json::from_str(&row.intended_payload_json)?;
    readback.submitted_at_local =
        super::record::local_timestamp(readback.submitted_at.as_deref(), &intent.zone()?)?;
    let mut receipt: serde_json::Value = serde_json::from_str(
        row.receipt_record_json
            .as_deref()
            .ok_or(JournalError::StateConflict)?,
    )?;
    receipt["readback"] = serde_json::to_value(&readback)?;
    if receipt
        .get("text")
        .is_some_and(serde_json::Value::is_object)
    {
        receipt["text"]["server_body_sha256"] = serde_json::to_value(
            posted
                .body_sha256
                .as_ref()
                .or(readback.body_sha256.as_ref()),
        )?;
    }
    let receipt = receipt.to_string();
    let readback = if let Some(full) = full {
        let mut value = full_readback(full)?;
        value["submitted_at_local"] = serde_json::to_value(&readback.submitted_at_local)?;
        value.to_string()
    } else {
        serde_json::to_string(&readback)?
    };
    let jid = journal_id.to_owned();
    store.call_blocking(move |conns| {
        let tx = conns.state.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute("UPDATE submission_journal SET readback_record_json = ?1, receipt_record_json = ?2 WHERE journal_id = ?3 AND state = ?4",
            params![readback, receipt, jid, row.state.as_str()])?;
        if changed != 1 { return Err(DbError::Message("state conflict".into())); }
        tx.commit()?;
        Ok(())
    }).map_err(JournalError::from)
}

/// Apply an explicit user assumption after a current history read finds no newer attempt.
pub fn assume_not_submitted(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    newer_attempt_visible: bool,
    now: Timestamp,
) -> Result<(), JournalError> {
    let row = get_journal(store, journal_id)?.ok_or(JournalError::NotFound)?;
    let started: Timestamp = row
        .posting_started_at
        .as_deref()
        .ok_or(JournalError::StateConflict)?
        .parse()
        .map_err(|_| JournalError::StateConflict)?;
    verify_store_directory(store, owner.identity_dir())?;
    if !owner.matches(journal_id)
        || row.state != State::OutcomeUnknown
        || newer_attempt_visible
        || now.as_nanosecond() - started.as_nanosecond() < 1_800_000_000_000
    {
        return Err(JournalError::StateConflict);
    }
    let jid = journal_id.to_owned();
    store.call_blocking(move |conns| {
        let tx = conns.state.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute("UPDATE submission_journal SET state = 'uploaded_not_submitted', uploaded_not_submitted_at = ?1,
            terminal_at = ?1, not_submitted_evidence = 'assumed' WHERE journal_id = ?2 AND state = 'outcome_unknown'", params![now.to_string(), jid])?;
        if changed != 1 { return Err(DbError::Message("state conflict".into())); }
        bump_journal_epochs(&tx, &jid)?;
        crate::events::record_submission_state(&tx, &jid, Some("outcome_unknown"), "uploaded_not_submitted")?;
        tx.commit()?;
        Ok(())
    }).map_err(JournalError::from)
}

/// Owner-absent recovery. `Ok(None)` if owner is live.
pub fn recover_if_owner_absent(
    store: &Store,
    identity_dir: &Path,
    journal_id: &str,
) -> Result<Option<State>, JournalError> {
    verify_store_directory(store, identity_dir)?;
    let Some(owner) = OwnerLock::try_acquire(identity_dir, journal_id)? else {
        return Ok(None);
    };
    recover_owned(store, &owner, journal_id).map(Some)
}

/// Recover a stale operation after the caller acquired its absent owner's lock.
pub fn recover_owned(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
) -> Result<State, JournalError> {
    verify_store_directory(store, owner.identity_dir())?;
    if !owner.matches(journal_id) {
        return Err(JournalError::StateConflict);
    }
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
        other => return Ok(other),
    };
    transition(store, owner, journal_id, row.state, to, patch)?;
    Ok(to)
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
                        acknowledged_at, error_text, identity_key, intended_payload_json, baseline_attempt, baseline_submission_id, created_at, planned_at, uploading_at, uploaded_at, posting_at, posting_started_at, submitted_at, matched_at, terminal_at, upload_incomplete_at, uploaded_not_submitted_at, outcome_unknown_at, refused_at, post_status, response_kind, not_submitted_evidence, readback_record_json, server_match_json, plan_id, approval_json
                 FROM submission_journal WHERE journal_id = ?1",
                params![jid],
                |r| {
                    Ok(JournalRow {
                        identity_key: r.get(10)?,
                        intended_payload_json: r.get(11)?,
                        baseline_attempt: r.get(12)?,
                        baseline_submission_id: r.get(13)?,
                        created_at: r.get(14)?,
                        planned_at: r.get(15)?,
                        uploading_at: r.get(16)?,
                        uploaded_at: r.get(17)?,
                        posting_at: r.get(18)?,
                        posting_started_at: r.get(19)?,
                        submitted_at: r.get(20)?,
                        matched_at: r.get(21)?,
                        terminal_at: r.get(22)?,
                        upload_incomplete_at: r.get(23)?,
                        uploaded_not_submitted_at: r.get(24)?,
                        outcome_unknown_at: r.get(25)?,
                        refused_at: r.get(26)?,
                        post_status: r.get(27)?,
                        response_kind: r.get(28)?,
                        not_submitted_evidence: r.get(29)?,
                        readback_record_json: r.get(30)?,
                        server_match_json: r.get(31)?,
                        journal_id: r.get(0)?,
                        state: State::from_str(&r.get::<_, String>(1)?).map_err(|()| rusqlite::Error::InvalidQuery)?,
                        assignment_id: r.get(2)?,
                        course_id: r.get(3)?,
                        kind: r.get(4)?,
                        uploaded_file_ids_json: r.get(5)?,
                        response_record_json: r.get(6)?,
                        receipt_record_json: r.get(7)?,
                        acknowledged_at: r.get(8)?,
                        error_text: r.get(9)?,
                        plan_id: r.get(32)?,
                        approval_json: r.get(33)?,
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
        let tx = conns
            .state
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
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
pub fn owner_status_for(
    identity_dir: &Path,
    journal_id: &str,
    state: State,
) -> Result<OwnerStatus, LockError> {
    if state.is_terminal() {
        Ok(OwnerStatus::NotApplicable)
    } else {
        probe_owner(identity_dir, journal_id)
    }
}

fn verify_store_directory(store: &Store, identity_dir: &Path) -> Result<(), JournalError> {
    let expected = std::fs::canonicalize(identity_dir)?;
    let actual = store.call_blocking(|c| {
        Ok(c.state
            .query_row("PRAGMA database_list", [], |r| r.get::<_, String>(2))?)
    })?;
    let actual = Path::new(&actual)
        .parent()
        .ok_or(JournalError::StateConflict)?;
    if std::fs::canonicalize(actual)? != expected {
        return Err(JournalError::StateConflict);
    }
    Ok(())
}

fn bump_journal_epochs(tx: &rusqlite::Transaction<'_>, jid: &str) -> Result<(), DbError> {
    let (course, assignment): (i64, i64) = tx.query_row(
        "SELECT course_id, assignment_id FROM submission_journal WHERE journal_id = ?1",
        [jid],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let scopes = [
        format!("submission:assignment:{assignment}"),
        format!("assignments:course:{course}"),
        format!("assignment_groups:course:{course}:*"),
        "missing:all".into(),
        "planner:*".into(),
        "enrollment_grades:*".into(),
        format!("course_totals:course:{course}"),
    ];
    bump_epochs(&scopes.iter().map(String::as_str).collect::<Vec<_>>(), tx)
}

fn validate_patch(mut patch: TransitionPatch) -> Result<TransitionPatch, JournalError> {
    if let Some(raw) = &mut patch.server_match_json {
        *raw = serde_json::to_string(&serde_json::from_str::<Option<super::CandidateRecord>>(
            raw,
        )?)?;
    }
    patch.error_text = patch.error_text.map(|s| canvas_api::redact::redact(&s));
    Ok(patch)
}

/// Whether a later confirmed operation supersedes this unknown outcome.
pub fn is_superseded(store: &Store, journal_id: &str) -> Result<bool, JournalError> {
    let row = get_journal(store, journal_id)?.ok_or(JournalError::NotFound)?;
    if row.state != State::OutcomeUnknown {
        return Ok(false);
    }
    let created: Timestamp = row
        .created_at
        .parse()
        .map_err(|_| JournalError::StateConflict)?;
    Ok(store.call_blocking(move |conns| {
        let mut stmt = conns.state.prepare("SELECT created_at FROM submission_journal WHERE assignment_id = ?1 AND state IN ('submitted','matched')")?;
        for at in stmt.query_map([row.assignment_id], |r| r.get::<_, String>(0))? {
            let at: Timestamp = at?.parse().map_err(|_| DbError::Message("invalid timestamp".into()))?;
            if at > created { return Ok(true); }
        }
        Ok(false)
    })?)
}

fn build_receipt(
    tx: &rusqlite::Transaction<'_>,
    jid: &str,
    receipt: &ReceiptRecord,
) -> Result<String, DbError> {
    use serde_json::{Value, json};
    #[allow(clippy::type_complexity)]
    let (course, assignment, kind, baseline, created, payload, plan_id, approval_json): (
        i64,
        i64,
        String,
        Option<i64>,
        String,
        String,
        Option<String>,
        Option<String>,
    ) = tx.query_row(
        "SELECT course_id, assignment_id, kind, baseline_attempt, created_at,
                intended_payload_json, plan_id, approval_json
         FROM submission_journal WHERE journal_id = ?1",
        [jid],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
            ))
        },
    )?;
    let approval: Option<Value> = approval_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| DbError::Message("invalid approval".into()))?;
    let intent: Value =
        serde_json::from_str(&payload).map_err(|_| DbError::Message("invalid intent".into()))?;
    let identity = |key| {
        tx.query_row("SELECT value FROM identity WHERE key = ?1", [key], |r| {
            r.get::<_, String>(0)
        })
    };
    let files: Vec<Value> = intent.get("files").and_then(Value::as_array).into_iter().flatten().map(|f| json!({
        "name": f.get("name").and_then(Value::as_str), "size": f.get("size").and_then(Value::as_u64),
        "sha256": f.get("sha256").and_then(Value::as_str), "canvas_file_id": f.get("canvas_file_id").and_then(Value::as_str),
    })).collect();
    let text = intent.get("text").filter(|v| v.is_object()).map(|t| {
        json!({
            "input_sha256": t.get("input_sha256").and_then(Value::as_str),
            "transform": t.get("transform").and_then(Value::as_str),
            "sent_sha256": t.get("sent_sha256").and_then(Value::as_str),
            "server_body_sha256": receipt.posted.body_sha256,
        })
    });
    Ok(json!({
        "receipt_id": receipt.receipt_id, "journal_id": jid,
        "identity": { "origin": identity("origin")?, "user_id": identity("user_id")?, "key": identity("key")? },
        "course_id": course.to_string(), "course_code": intent.get("course_code").and_then(Value::as_str),
        "assignment_id": assignment.to_string(), "assignment_name": intent.get("assignment_name").and_then(Value::as_str),
        "kind": kind, "baseline_attempt": baseline.unwrap_or(0), "created_at": created,
        "attribution": receipt.attribution, "posted": receipt.posted, "readback": receipt.readback,
        "files": files, "text": text, "url": intent.get("url").and_then(Value::as_str),
        "due_at": intent.get("due_at").and_then(Value::as_str), "cli_version": env!("CARGO_PKG_VERSION"),
        "plan_id": plan_id, "approval": approval,
    }).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{IdentityDocument, Paths};
    use crate::journal::locks::AdmissionLock;
    use crate::journal::record::{Evidence, allowlist_from_json};
    use crate::store::OpenIdentity;
    use serde_json::json;
    use std::sync::{Arc, Barrier};
    use std::thread;
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
        let admission = AdmissionLock::try_acquire(&paths.identity_dir, 42).unwrap();
        let (jid, owner) =
            create(&store, &paths.identity_dir, &admission, &opts_for(&doc, 42)).unwrap();
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
            let admission = AdmissionLock::try_acquire(&paths.identity_dir, assignment).unwrap();
            let (jid, owner) = create(
                &store,
                &paths.identity_dir,
                &admission,
                &opts_for(&doc, assignment),
            )
            .unwrap();
            if from == State::Posting {
                transition(
                    &store,
                    &owner,
                    &jid,
                    State::Planned,
                    State::Uploaded,
                    TransitionPatch::default(),
                )
                .unwrap();
            }
            transition(
                &store,
                &owner,
                &jid,
                if from == State::Posting {
                    State::Uploaded
                } else {
                    State::Planned
                },
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
        let admission = AdmissionLock::try_acquire(&paths.identity_dir, 7).unwrap();
        let (jid, _owner) =
            create(&store, &paths.identity_dir, &admission, &opts_for(&doc, 7)).unwrap();
        assert!(
            recover_if_owner_absent(&store, &paths.identity_dir, &jid)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            owner_status_for(&paths.identity_dir, &jid, State::Planned).unwrap(),
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
                    Ok(admission) => create(&store, &idir, &admission, &opts).map(|_| true),
                    Err(LockError::InProgress) => Ok(false),
                    Err(e) => Err(JournalError::from(e)),
                }
            }));
        }
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
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
        let admission = AdmissionLock::try_acquire(&paths.identity_dir, 11).unwrap();
        let (jid, owner) =
            create(&store, &paths.identity_dir, &admission, &opts_for(&doc, 11)).unwrap();
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
        let admission = AdmissionLock::try_acquire(&paths.identity_dir, 3).unwrap();
        let (jid, owner) =
            create(&store, &paths.identity_dir, &admission, &opts_for(&doc, 3)).unwrap();
        transition(
            &store,
            &owner,
            &jid,
            State::Planned,
            State::Uploading,
            TransitionPatch::default(),
        )
        .unwrap();
        transition(
            &store,
            &owner,
            &jid,
            State::Uploading,
            State::Uploaded,
            TransitionPatch::default(),
        )
        .unwrap();
        mark_posting(&store, &owner, &jid).unwrap();
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
        commit_success(&store, &owner, &jid, 201, &receipt).unwrap();
        let row = get_journal(&store, &jid).unwrap().unwrap();
        assert_eq!(row.state, State::Submitted);
        assert!(row.receipt_record_json.is_some());
        assert!(row.response_record_json.is_some());
    }
}
