//! Operation journal storage, transitions, and recovery (M8-b).
//!
//! Every rule here is SPEC §12.2's, applied to an operation instead of a
//! submission:
//!
//! - the caller holds the target admission lock before the insert, and the
//!   owner lock is taken before the row is published;
//! - every state change is one `BEGIN IMMEDIATE` transaction with an
//!   expected-state guard, so zero rows affected means another actor moved
//!   first;
//! - the journal row, its event, and the cache epochs it invalidates commit
//!   together or not at all;
//! - an ambiguous `POST` becomes `outcome_unknown` and is never resent. Only
//!   `operation reconcile` moves it.

use std::path::Path;
use std::str::FromStr;

use jiff::Timestamp;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::journal::{AdmissionLock, LockError, OwnerLock, OwnerStatus, probe_owner};
use crate::plan::PlanRow;
use crate::store::{DbError, Store, bump_epochs};

use super::OperationError;
use super::record::{
    Attribution, NotPostedEvidence, OpState, OperationKind, OperationPlan, OperationReadback,
    OperationRow, OperationTarget, ResponseRecord, ServerMatch,
};

/// How long an unknown outcome must stand before it can be assumed away.
///
/// The same thirty minutes SPEC §12.2 sets for a submission, and for the same
/// reason: a slow Canvas can still land a request minutes after the client
/// gave up on it.
pub const ASSUME_AFTER: jiff::SignedDuration = jiff::SignedDuration::from_mins(30);

const COLUMNS: &str = "journal_id, identity_key, plan_id, kind, intended_json, approval_json,
     state, post_status, response_kind, not_posted_evidence, attribution,
     response_record_json, readback_json, server_match_json, receipt_record_json,
     uploaded_file_ids_json, error_text, created_at, posting_started_at, terminal_at,
     acknowledged_at";

fn decode(what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(DbError::Message(format!("unreadable operation {what}"))),
    )
}

/// Decode one nullable JSON column.
fn json_of<T: serde::de::DeserializeOwned>(
    raw: Option<String>,
    what: &'static str,
) -> Result<Option<T>, rusqlite::Error> {
    raw.map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|_| decode(what))
}

/// Every target column of one operation, in the order the table declares them.
type TargetColumns = (
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<i64>,
);

fn row_from(row: &rusqlite::Row<'_>) -> Result<OperationRow, rusqlite::Error> {
    let kind: String = row.get(3)?;
    let intended: String = row.get(4)?;
    let approval: Option<String> = row.get(5)?;
    let state: String = row.get(6)?;
    let evidence: Option<String> = row.get(9)?;
    let attribution: String = row.get(10)?;

    Ok(OperationRow {
        journal_id: row.get(0)?,
        identity_key: row.get(1)?,
        plan_id: row.get(2)?,
        kind: OperationKind::from_str(&kind).map_err(|()| decode("kind"))?,
        intended: serde_json::from_str(&intended).map_err(|_| decode("intent"))?,
        approval: json_of(approval, "approval")?,
        state: OpState::from_str(&state).map_err(|()| decode("state"))?,
        post_status: row.get(7)?,
        response_kind: row.get(8)?,
        not_posted_evidence: evidence
            .map(|raw| NotPostedEvidence::from_str(&raw))
            .transpose()
            .map_err(|()| decode("not_posted_evidence"))?,
        attribution: Attribution::from_str(&attribution).map_err(|()| decode("attribution"))?,
        response: json_of(row.get(11)?, "response")?,
        readback: json_of(row.get(12)?, "readback")?,
        server_match: json_of(row.get(13)?, "server match")?,
        receipt: json_of(row.get(14)?, "receipt")?,
        uploaded_file_ids: serde_json::from_str(&row.get::<_, String>(15)?)
            .map_err(|_| decode("uploaded ids"))?,
        error_text: row.get(16)?,
        created_at: row.get(17)?,
        posting_started_at: row.get(18)?,
        terminal_at: row.get(19)?,
        acknowledged_at: row.get(20)?,
    })
}

/// Create a `planned` operation journal and link its approved plan.
///
/// Caller holds the target admission lock. The insert, the plan's move to
/// `executed`, and the event are one transaction, so nothing uploads or posts
/// before a journal exists to record it.
pub fn create_linked(
    store: &Store,
    identity_dir: &Path,
    admission: &AdmissionLock,
    plan: &PlanRow,
) -> Result<(String, OwnerLock), OperationError> {
    let intended = plan
        .operation
        .clone()
        .ok_or(OperationError::StateConflict)?;
    let approval = plan.approval.clone().ok_or(OperationError::StateConflict)?;
    if !admission.matches_named(identity_dir, &intended.target.admission_name(&plan.plan_id)) {
        return Err(OperationError::StateConflict);
    }
    verify_store_directory(store, identity_dir)?;

    let journal_id = Uuid::new_v4().to_string();
    let owner = OwnerLock::acquire(identity_dir, &journal_id)?;
    #[cfg(test)]
    super::crash_tests::checkpoint("owner_acquired", &journal_id);

    let now = Timestamp::now().to_string();
    let jid = journal_id.clone();
    let plan_id = plan.plan_id.clone();
    let identity_key = plan.identity_key.clone();
    let kind = intended.kind();
    let scope = intended.target.event_scope();
    let (course_id, topic_id, parent_entry_id, conversation_id, recipients, quiz_id) =
        target_columns(&intended.target);
    let subject = intended.subject.clone();
    let input_sha256 = intended.body.input_sha256.clone();
    let sent_sha256 = intended.body.sent_sha256.clone();
    let intended_json = serde_json::to_string(&intended)?;
    let approval_json = serde_json::to_string(&approval)?;

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
            "INSERT INTO operation_journal (
                journal_id, identity_key, plan_id, kind, course_id, topic_id,
                parent_entry_id, conversation_id, quiz_id, recipients_json, subject,
                input_sha256, sent_sha256, intended_json, approval_json, state,
                attribution, created_at, planned_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,'planned','none',?16,?16)",
            params![
                jid,
                identity_key,
                plan_id,
                kind.as_str(),
                 course_id,
                 topic_id,
                 parent_entry_id,
                 conversation_id,
                 quiz_id,
                 recipients,
                 subject,
                 input_sha256,
                 sent_sha256,
                 intended_json,
                 approval_json,
                 now,
            ],
        );
        match insert {
            Ok(_) => {
                // The plan's own state is the guard: a second execute of the
                // same plan finds it `executed` and creates nothing.
                let linked = tx.execute(
                    "UPDATE plans SET state = 'executed', journal_id = ?1
                     WHERE plan_id = ?2 AND state = 'approved'",
                    params![jid, plan_id],
                )?;
                if linked != 1 {
                    return Err(DbError::Message("state conflict".into()));
                }
                crate::events::record_operation_state(&tx, &jid, &scope, None, "planned")?;
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
                Err(DbError::Message("in_progress".into()))
            }
            Err(e) => Err(DbError::from(e)),
        }
    });

    match result {
        Ok(()) => Ok((journal_id, owner)),
        Err(DbError::Message(message)) if message == "in_progress" => {
            Err(OperationError::InProgress)
        }
        Err(e) => Err(OperationError::from(e)),
    }
}

fn target_columns(target: &OperationTarget) -> TargetColumns {
    match target {
        OperationTarget::DiscussionReply {
            course_id,
            topic_id,
            parent_entry_id,
        } => (
            Some(*course_id),
            Some(*topic_id),
            *parent_entry_id,
            None,
            None,
            None,
        ),
        OperationTarget::InboxSend { recipients } => (
            None,
            None,
            None,
            None,
            Some(serde_json::to_string(recipients).unwrap_or_else(|_| "[]".to_owned())),
            None,
        ),
        OperationTarget::InboxReply { conversation_id } => {
            (None, None, None, Some(*conversation_id), None, None)
        }
        OperationTarget::QuizSubmit {
            course_id, quiz_id, ..
        } => (Some(*course_id), None, None, None, None, Some(*quiz_id)),
    }
}

/// Read one operation journal.
pub fn get(store: &Store, journal_id: &str) -> Result<Option<OperationRow>, OperationError> {
    let jid = journal_id.to_owned();
    Ok(store.call_blocking(move |conns| {
        Ok(conns
            .state
            .query_row(
                &format!("SELECT {COLUMNS} FROM operation_journal WHERE journal_id = ?1"),
                [&jid],
                row_from,
            )
            .optional()?)
    })?)
}

/// Read one operation journal or fail.
pub fn require(store: &Store, journal_id: &str) -> Result<OperationRow, OperationError> {
    get(store, journal_id)?.ok_or(OperationError::NotFound)
}

/// The operation journal a plan admitted, when it admitted one.
pub fn for_plan(store: &Store, plan_id: &str) -> Result<Option<OperationRow>, OperationError> {
    let key = plan_id.to_owned();
    Ok(store.call_blocking(move |conns| {
        Ok(conns
            .state
            .query_row(
                &format!("SELECT {COLUMNS} FROM operation_journal WHERE plan_id = ?1"),
                [&key],
                row_from,
            )
            .optional()?)
    })?)
}

/// Every operation journal, newest first.
pub fn list(store: &Store, state: Option<OpState>) -> Result<Vec<OperationRow>, OperationError> {
    let filter = state.map(|s| s.as_str().to_owned());
    Ok(store.call_blocking(move |conns| {
        let sql = match &filter {
            Some(_) => format!(
                "SELECT {COLUMNS} FROM operation_journal WHERE state = ?1
                 ORDER BY created_at DESC, journal_id ASC"
            ),
            None => format!(
                "SELECT {COLUMNS} FROM operation_journal
                 ORDER BY created_at DESC, journal_id ASC"
            ),
        };
        let mut stmt = conns.state.prepare(&sql)?;
        let rows = match &filter {
            Some(state) => stmt
                .query_map([state], row_from)?
                .collect::<Result<_, _>>()?,
            None => stmt.query_map([], row_from)?.collect::<Result<_, _>>()?,
        };
        Ok(rows)
    })?)
}

/// Optional column patches for an operation transition.
#[derive(Debug, Clone, Default)]
pub struct Patch {
    /// Error text; it is redacted before it is stored.
    pub error_text: Option<String>,
    /// HTTP status of the `POST`.
    pub post_status: Option<i64>,
    /// How the response was classified: `canvas-error`, `other`, or `none`.
    pub response_kind: Option<&'static str>,
    /// Why the operation is known not to have been posted.
    pub not_posted_evidence: Option<NotPostedEvidence>,
}

/// Whether a transition is one the state machine allows.
const fn allowed(from: OpState, to: OpState) -> bool {
    matches!(
        (from, to),
        (OpState::Planned, OpState::Posting | OpState::Refused)
            | (
                OpState::Posting,
                OpState::OutcomeUnknown | OpState::Failed | OpState::Refused
            )
            | (OpState::OutcomeUnknown, OpState::OutcomeUnknown)
    )
}

/// Guarded state transition. Confirmed outcomes use the commit helpers below.
pub fn transition(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    from: OpState,
    to: OpState,
    patch: Patch,
) -> Result<(), OperationError> {
    verify_store_directory(store, owner.identity_dir())?;
    if !owner.matches(journal_id) || !allowed(from, to) {
        return Err(OperationError::StateConflict);
    }
    if to == OpState::Refused && patch.not_posted_evidence.is_none() {
        // A `refused` operation is one that was never sent, and the row has to
        // say how that is known.
        return Err(OperationError::StateConflict);
    }
    let row = require(store, journal_id)?;
    let scope = row.intended.target.event_scope();
    let jid = journal_id.to_owned();
    let now = Timestamp::now().to_string();
    let ts_col = format!("{}_at", to.as_str());
    let error_text = patch
        .error_text
        .map(|text| canvas_api::redact::redact(&text));

    store
        .call_blocking(move |conns| {
            let tx = conns
                .state
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let sql = format!(
                "UPDATE operation_journal SET state = ?1, {ts_col} = ?2,
                 terminal_at = CASE WHEN ?3 THEN ?2 ELSE terminal_at END,
                 posting_started_at = CASE WHEN ?4 THEN ?2 ELSE posting_started_at END,
                 error_text = COALESCE(?5, error_text),
                 response_kind = COALESCE(?6, response_kind),
                 not_posted_evidence = COALESCE(?7, not_posted_evidence),
                 post_status = COALESCE(?8, post_status)
                 WHERE journal_id = ?9 AND state = ?10"
            );
            let changed = tx.execute(
                &sql,
                params![
                    to.as_str(),
                    now,
                    to.is_terminal(),
                    to == OpState::Posting,
                    error_text,
                    patch.response_kind,
                    patch.not_posted_evidence.map(NotPostedEvidence::as_str),
                    patch.post_status,
                    jid,
                    from.as_str(),
                ],
            )?;
            if changed != 1 {
                return Err(DbError::Message("state conflict".into()));
            }
            crate::events::record_operation_state(
                &tx,
                &jid,
                &scope,
                Some(from.as_str()),
                to.as_str(),
            )?;
            tx.commit()?;
            Ok(())
        })
        .map_err(OperationError::from)
}

/// `planned` → `posting`, recording when the `POST` started.
pub fn mark_posting(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
) -> Result<(), OperationError> {
    transition(
        store,
        owner,
        journal_id,
        OpState::Planned,
        OpState::Posting,
        Patch::default(),
    )
}

/// Record a Canvas file id against one frozen attachment, before the `POST`.
pub fn record_attachment_id(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    index: usize,
    file_id: i64,
) -> Result<(), OperationError> {
    verify_store_directory(store, owner.identity_dir())?;
    if !owner.matches(journal_id) || file_id <= 0 {
        return Err(OperationError::StateConflict);
    }
    let jid = journal_id.to_owned();
    store.call_blocking(move |conns| {
        let tx = conns
            .state
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (intended_json, ids_json): (String, String) = tx
            .query_row(
                "SELECT intended_json, uploaded_file_ids_json FROM operation_journal
                 WHERE journal_id = ?1 AND state = 'planned'",
                [&jid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| DbError::Message("state conflict".into()))?;
        let mut intended: OperationPlan = serde_json::from_str(&intended_json)
            .map_err(|_| DbError::Message("invalid intent".into()))?;
        let attachment = intended
            .attachments
            .get_mut(index)
            .ok_or_else(|| DbError::Message("invalid attachment index".into()))?;
        if attachment.canvas_file_id.is_some() {
            return Err(DbError::Message("attachment already uploaded".into()));
        }
        attachment.canvas_file_id = Some(file_id.to_string());
        let mut ids: Vec<String> = serde_json::from_str(&ids_json)
            .map_err(|_| DbError::Message("invalid uploaded ids".into()))?;
        let id = file_id.to_string();
        if ids.contains(&id) {
            return Err(DbError::Message("invalid uploaded id".into()));
        }
        ids.push(id);
        tx.execute(
            "UPDATE operation_journal SET intended_json = ?1, uploaded_file_ids_json = ?2
             WHERE journal_id = ?3 AND state = 'planned'",
            params![
                serde_json::to_string(&intended).map_err(|e| DbError::Message(e.to_string()))?,
                serde_json::to_string(&ids).map_err(|e| DbError::Message(e.to_string()))?,
                jid,
            ],
        )?;
        tx.commit()?;
        Ok(())
    })?;
    Ok(())
}

/// Store an observed 2xx, its receipt, and the epochs it invalidates.
///
/// The attribution is `accepted` when the response named an object id and
/// `none` when it did not: a 2xx alone says Canvas took the request, and this
/// process never claims more than that (REPORT §3.5).
pub fn commit_posted(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    post_status: u16,
    response: &ResponseRecord,
) -> Result<(), OperationError> {
    if !(200..300).contains(&post_status) {
        return Err(OperationError::StateConflict);
    }
    let attribution = if response.id.is_some() {
        Attribution::Accepted
    } else {
        Attribution::None
    };
    commit(
        store,
        owner,
        journal_id,
        OpState::Posting,
        OpState::Posted,
        &Committed {
            post_status: Some(i64::from(post_status)),
            attribution,
            response: Some(response.clone()),
            readback: None,
            server_match: None,
        },
    )
}

/// Store a digest-only match found by reconcile: state `matched`, `unproven`.
pub fn commit_matched(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    server_match: &ServerMatch,
    readback: &OperationReadback,
) -> Result<(), OperationError> {
    commit(
        store,
        owner,
        journal_id,
        OpState::OutcomeUnknown,
        OpState::Matched,
        &Committed {
            post_status: None,
            attribution: Attribution::Unproven,
            response: None,
            readback: Some(readback.clone()),
            server_match: Some(server_match.clone()),
        },
    )
}

/// Record a quiz operation whose readback shows its own completion.
///
/// A quiz readback names the object by id, shows it completed at the frozen
/// attempt, and digests the answers it holds to the ones the operation sent.
/// That is `posted` with `observed`, never `matched`: the submission the
/// journal attempted is the one Canvas holds. Quiz only; every other
/// operation resolves an unknown outcome through `commit_matched`.
pub fn commit_observed(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    server_match: &ServerMatch,
    readback: &OperationReadback,
) -> Result<(), OperationError> {
    commit(
        store,
        owner,
        journal_id,
        OpState::OutcomeUnknown,
        OpState::Posted,
        &Committed {
            post_status: None,
            attribution: Attribution::Observed,
            response: None,
            readback: Some(readback.clone()),
            server_match: Some(server_match.clone()),
        },
    )
}

struct Committed {
    post_status: Option<i64>,
    attribution: Attribution,
    response: Option<ResponseRecord>,
    readback: Option<OperationReadback>,
    server_match: Option<ServerMatch>,
}

fn commit(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    from: OpState,
    to: OpState,
    what: &Committed,
) -> Result<(), OperationError> {
    verify_store_directory(store, owner.identity_dir())?;
    if !owner.matches(journal_id) {
        return Err(OperationError::StateConflict);
    }
    let mut row = require(store, journal_id)?;
    row.state = to;
    row.post_status = what.post_status.or(row.post_status);
    row.attribution = what.attribution;
    row.response.clone_from(&what.response);
    row.readback.clone_from(&what.readback);
    row.server_match.clone_from(&what.server_match);

    let scope = row.intended.target.event_scope();
    let epochs = row.intended.target.epoch_scopes();
    let receipt = super::receipt::build(&row, Uuid::new_v4().to_string());
    let receipt_json = serde_json::to_string(&receipt)?;
    let response_json = what
        .response
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let readback_json = what
        .readback
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let match_json = what
        .server_match
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let jid = journal_id.to_owned();
    let post_status = what.post_status;
    let attribution = what.attribution;
    let now = Timestamp::now().to_string();
    let ts_col = format!("{}_at", to.as_str());

    store
        .call_blocking(move |conns| {
            let tx = conns
                .state
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let sql = format!(
                "UPDATE operation_journal SET state = ?1, {ts_col} = ?2, terminal_at = ?2,
                 post_status = COALESCE(?3, post_status), attribution = ?4,
                 response_record_json = COALESCE(?5, response_record_json),
                 readback_json = COALESCE(?6, readback_json),
                 server_match_json = COALESCE(?7, server_match_json),
                 receipt_record_json = ?8
                 WHERE journal_id = ?9 AND state = ?10"
            );
            let changed = tx.execute(
                &sql,
                params![
                    to.as_str(),
                    now,
                    post_status,
                    attribution.as_str(),
                    response_json,
                    readback_json,
                    match_json,
                    receipt_json,
                    jid,
                    from.as_str(),
                ],
            )?;
            if changed != 1 {
                return Err(DbError::Message("state conflict".into()));
            }
            bump_epochs(&epochs.iter().map(String::as_str).collect::<Vec<_>>(), &tx)?;
            crate::events::record_operation_state(
                &tx,
                &jid,
                &scope,
                Some(from.as_str()),
                to.as_str(),
            )?;
            #[cfg(test)]
            super::crash_tests::checkpoint("commit_before_commit", &jid);
            tx.commit()?;
            #[cfg(test)]
            super::crash_tests::checkpoint("commit_after_commit", &jid);
            Ok(())
        })
        .map_err(OperationError::from)
}

/// Record what the answers POST answered, while the journal is `posting`.
///
/// A quiz sends two requests; the answers response is an observation worth
/// keeping even when the completion that follows it is never observed. The
/// guard is the state: only a journal still `posting` takes it, and the
/// completion's own record overwrites it when it arrives. Quiz only.
pub fn record_answers_response(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    response: &ResponseRecord,
) -> Result<(), OperationError> {
    verify_store_directory(store, owner.identity_dir())?;
    if !owner.matches(journal_id) {
        return Err(OperationError::StateConflict);
    }
    let json = serde_json::to_string(response).map_err(OperationError::Json)?;
    let jid = journal_id.to_owned();
    store
        .call_blocking(move |conns| {
            let changed = conns.state.execute(
                "UPDATE operation_journal SET response_record_json = ?1
                 WHERE journal_id = ?2 AND state = 'posting'",
                rusqlite::params![json, jid],
            )?;
            if changed != 1 {
                return Err(DbError::Message("state conflict".into()));
            }
            Ok(())
        })
        .map_err(OperationError::from)
}
/// Record a readback against a journal without changing its state.
///
/// This is how `accepted` becomes `observed`: the readback showed the id the
/// acceptance named, so the object is no longer only something Canvas said it
/// created. The state never moves here, and neither does a journal whose
/// readback shows nothing.
pub fn enrich_readback(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    readback: &OperationReadback,
) -> Result<Attribution, OperationError> {
    verify_store_directory(store, owner.identity_dir())?;
    if !owner.matches(journal_id) {
        return Err(OperationError::StateConflict);
    }
    let mut row = require(store, journal_id)?;
    let observed = matches!(
        (&row.response, &readback.id),
        (Some(response), Some(found))
            if response.id.as_deref() == Some(found.as_str())
    );
    let attribution = if observed {
        Attribution::Observed
    } else {
        row.attribution
    };
    row.readback = Some(readback.clone());
    row.attribution = attribution;
    let receipt_json = row
        .state
        .is_done()
        .then(|| {
            let id = row
                .receipt_id()
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            serde_json::to_string(&super::receipt::build(&row, id))
        })
        .transpose()?;
    let readback_json = serde_json::to_string(readback)?;
    let state = row.state.as_str();
    let jid = journal_id.to_owned();

    store.call_blocking(move |conns| {
        let tx = conns
            .state
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE operation_journal SET readback_json = ?1, attribution = ?2,
             receipt_record_json = COALESCE(?3, receipt_record_json)
             WHERE journal_id = ?4 AND state = ?5",
            params![
                readback_json,
                attribution.as_str(),
                receipt_json,
                jid,
                state
            ],
        )?;
        if changed != 1 {
            return Err(DbError::Message("state conflict".into()));
        }
        tx.commit()?;
        Ok(())
    })?;
    Ok(attribution)
}

/// Apply an explicit assumption that nothing was posted.
///
/// Refused unless the journal is `outcome_unknown`, a current readback found
/// nothing, and thirty minutes have passed since the `POST` started. The same
/// three guards SPEC §12.2 puts on `--assume-not-submitted`.
pub fn assume_not_posted(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    candidate_visible: bool,
    now: Timestamp,
) -> Result<(), OperationError> {
    let row = require(store, journal_id)?;
    let started: Timestamp = row
        .posting_started_at
        .as_deref()
        .ok_or(OperationError::StateConflict)?
        .parse()
        .map_err(|_| OperationError::StateConflict)?;
    if row.state != OpState::OutcomeUnknown
        || candidate_visible
        || now.as_nanosecond() - started.as_nanosecond() < ASSUME_AFTER.as_nanos()
    {
        return Err(OperationError::StateConflict);
    }
    verify_store_directory(store, owner.identity_dir())?;
    if !owner.matches(journal_id) {
        return Err(OperationError::StateConflict);
    }
    let scope = row.intended.target.event_scope();
    let jid = journal_id.to_owned();
    let at = now.to_string();
    store
        .call_blocking(move |conns| {
            let tx = conns
                .state
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let changed = tx.execute(
                "UPDATE operation_journal SET state = 'refused', refused_at = ?1,
                 terminal_at = ?1, not_posted_evidence = 'assumed'
                 WHERE journal_id = ?2 AND state = 'outcome_unknown'",
                params![at, jid],
            )?;
            if changed != 1 {
                return Err(DbError::Message("state conflict".into()));
            }
            crate::events::record_operation_state(
                &tx,
                &jid,
                &scope,
                Some("outcome_unknown"),
                "refused",
            )?;
            tx.commit()?;
            Ok(())
        })
        .map_err(OperationError::from)
}

/// Owner-absent recovery. `Ok(None)` when the owner is live.
pub fn recover_if_owner_absent(
    store: &Store,
    identity_dir: &Path,
    journal_id: &str,
) -> Result<Option<OpState>, OperationError> {
    verify_store_directory(store, identity_dir)?;
    let Some(owner) = OwnerLock::try_acquire(identity_dir, journal_id)? else {
        return Ok(None);
    };
    recover_owned(store, &owner, journal_id).map(Some)
}

/// Recover after the caller took the absent owner's lock.
///
/// The whole table, and it is deliberately short:
///
/// | State when the owner vanished | Recovered to | Why |
/// |---|---|---|
/// | `planned` | `refused`, `never_sent` | the `POST` had not started |
/// | `posting` | `outcome_unknown`, `none` | the outcome was never observed |
/// | anything terminal | unchanged | it already has an answer |
pub fn recover_owned(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
) -> Result<OpState, OperationError> {
    verify_store_directory(store, owner.identity_dir())?;
    if !owner.matches(journal_id) {
        return Err(OperationError::StateConflict);
    }
    let row = require(store, journal_id)?;
    let (to, patch) = match row.state {
        OpState::Planned => (
            OpState::Refused,
            Patch {
                error_text: Some("abandoned before posting".into()),
                not_posted_evidence: Some(NotPostedEvidence::NeverSent),
                ..Patch::default()
            },
        ),
        OpState::Posting => (
            OpState::OutcomeUnknown,
            Patch {
                response_kind: Some("none"),
                ..Patch::default()
            },
        ),
        other => return Ok(other),
    };
    transition(store, owner, journal_id, row.state, to, patch)?;
    Ok(to)
}

/// Recover every abandoned operation journal for one target before admitting a new one.
pub fn recover_active(
    store: &Store,
    identity_dir: &Path,
    target: &OperationTarget,
) -> Result<Vec<(String, OpState)>, OperationError> {
    let ids = active_for_target(store, target)?;
    let mut recovered = Vec::new();
    for id in ids {
        if let Some(state) = recover_if_owner_absent(store, identity_dir, &id)? {
            recovered.push((id, state));
        }
    }
    Ok(recovered)
}

/// Journal ids for this target that are not terminal.
fn active_for_target(
    store: &Store,
    target: &OperationTarget,
) -> Result<Vec<String>, OperationError> {
    let (course_id, topic_id, _, conversation_id, _, quiz_id) = target_columns(target);
    Ok(store.call_blocking(move |conns| {
        let mut stmt = conns.state.prepare(
            "SELECT journal_id FROM operation_journal
             WHERE state IN ('planned','posting')
               AND (topic_id IS ?1 AND conversation_id IS ?2 AND quiz_id IS ?3)
             ORDER BY created_at ASC",
        )?;
        let _ = course_id;
        Ok(stmt
            .query_map(params![topic_id, conversation_id, quiz_id], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?)
    })?)
}

/// Acknowledge an `outcome_unknown` operation (the §10 pending hook).
pub fn acknowledge(store: &Store, journal_id: &str) -> Result<String, OperationError> {
    let jid = journal_id.to_owned();
    let now = Timestamp::now().to_string();
    let stamped = now.clone();
    store.call_blocking(move |conns| {
        let tx = conns
            .state
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE operation_journal SET acknowledged_at = ?1
             WHERE journal_id = ?2 AND state = 'outcome_unknown'",
            params![now, jid],
        )?;
        if changed == 0 {
            return Err(DbError::Message("state conflict".into()));
        }
        tx.commit()?;
        Ok(())
    })?;
    Ok(stamped)
}

/// Whether a later operation supersedes this one. It never does.
///
/// A submission is superseded because a later attempt on the same assignment
/// replaces it: the assignment holds one current attempt. A reply and a
/// message are not like that. A second reply to a topic is a second post, and
/// a second conversation is a second conversation, so a later write says
/// nothing at all about whether an earlier one landed — and an unknown
/// outcome that a later write retired would leave a person with a message
/// they never learned the fate of, and a duplicate they never learned about.
///
/// The field stays, because `receipts list` prints one column for both kinds
/// of journal; for an operation it is always `false`. Only `receipts
/// acknowledge` and `operation reconcile` retire an unknown operation.
#[must_use]
pub const fn is_superseded() -> bool {
    false
}

/// Owner status for JSON: `n/a` when terminal, else a probe.
pub fn owner_status_for(
    identity_dir: &Path,
    journal_id: &str,
    state: OpState,
) -> Result<OwnerStatus, LockError> {
    if state.is_terminal() {
        Ok(OwnerStatus::NotApplicable)
    } else {
        probe_owner(identity_dir, journal_id)
    }
}

/// Every operation journal that still has no answer (the §10 pending hook).
///
/// Pending means `planned` or `posting`, or `outcome_unknown` that has not
/// been acknowledged. Nothing else clears it: no later write supersedes an
/// operation ([`is_superseded`]).
pub fn pending(store: &Store) -> Result<Vec<String>, OperationError> {
    let mut out = Vec::new();
    for row in list(store, None)? {
        let pending = match row.state {
            OpState::Planned | OpState::Posting => true,
            OpState::OutcomeUnknown => row.acknowledged_at.is_none(),
            _ => false,
        };
        if pending {
            out.push(row.journal_id);
        }
    }
    Ok(out)
}

/// The identity block a receipt carries.
pub(super) fn identity_block(store: &Store) -> Result<Value, OperationError> {
    Ok(store.call_blocking(|conns| {
        let value = |name: &str| -> Result<String, DbError> {
            Ok(conns
                .state
                .query_row("SELECT value FROM identity WHERE key = ?1", [name], |r| {
                    r.get::<_, String>(0)
                })
                .optional()?
                .unwrap_or_default())
        };
        Ok(json!({
            "origin": value("origin")?,
            "user_id": value("user_id")?,
            "key": value("key")?,
        }))
    })?)
}

/// The Canvas user id this identity is, when the store records one.
pub fn identity_user_id(store: &Store) -> Result<Option<String>, OperationError> {
    Ok(identity_block(store)?
        .get("user_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned))
}

/// Store the identity block on a receipt that was built without one.
pub fn attach_identity(store: &Store, receipt: &mut Value) -> Result<(), OperationError> {
    receipt["identity"] = identity_block(store)?;
    Ok(())
}

fn verify_store_directory(store: &Store, identity_dir: &Path) -> Result<(), OperationError> {
    let expected = std::fs::canonicalize(identity_dir)?;
    let actual = store.call_blocking(|c| {
        Ok(c.state
            .query_row("PRAGMA database_list", [], |r| r.get::<_, String>(2))?)
    })?;
    let actual = Path::new(&actual)
        .parent()
        .ok_or(OperationError::StateConflict)?;
    if std::fs::canonicalize(actual)? != expected {
        return Err(OperationError::StateConflict);
    }
    Ok(())
}
