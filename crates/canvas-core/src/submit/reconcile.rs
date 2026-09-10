//! Shared reconcile logic (§12.2).

use std::collections::BTreeSet;

use canvas_api::models::{Submission, SubmissionHistoryEntry};
use canvas_api::{Client, get_submission_history};
use jiff::Timestamp;
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::identity::Paths;
use crate::journal::{
    CandidateRecord, Evidence, IntendedPayload, JournalError, JournalRow, LockError, OwnerLock,
    OwnerStatus, PostedRecord, ReadbackRecord, ReceiptRecord, ResponseKind, State, TransitionPatch,
    allowlist_from_json, assume_not_submitted, commit_matched, enrich_readback, get_journal,
    owner_status_for, recover_if_owner_absent, transition,
};
use crate::receipts::{export, rebuild_from_journal};
use crate::store::Store;
use crate::submit::freeze::hex_sha256;

/// Five-minute lookback from `posting_started_at`.
const WINDOW_NS: i128 = 300_000_000_000;
/// Thirty minutes before `--assume-not-submitted`.
const ASSUME_NS: i128 = 1_800_000_000_000;

/// Reconcile outcome label (§12.2 / Appendix D).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileOutcome {
    /// Transitioned to `matched` (or already confirmed).
    Ok,
    /// Still unresolved.
    Recovery,
    /// Ineligible.
    Refused,
}

/// Result of a reconcile run.
#[derive(Debug, Clone)]
pub struct ReconcileResult {
    /// Outcome label.
    pub outcome: ReconcileOutcome,
    /// Journal state after reconcile.
    pub state: State,
    /// Journal id.
    pub journal_id: String,
    /// Owner status.
    pub owner: OwnerStatus,
    /// Whether `--assume-not-submitted` is available.
    pub assume_available: bool,
    /// Attribution when confirmed.
    pub attribution: Option<String>,
    /// Receipt id when present.
    pub receipt_id: Option<String>,
    /// Posted record when present.
    pub posted: Option<PostedRecord>,
    /// Server match for text/URL.
    pub server_match: Option<CandidateRecord>,
    /// Window candidates.
    pub candidates: Vec<CandidateRecord>,
    /// Human message.
    pub message: String,
    /// Response kind when unknown.
    pub response_kind: Option<ResponseKind>,
    /// Not-submitted evidence when set.
    pub not_submitted_evidence: Option<String>,
}

/// Reconcile errors that map outside the structured result.
#[derive(Debug, Error)]
pub enum ReconcileError {
    #[error(transparent)]
    Network(#[from] canvas_api::Error),
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Lock(#[from] LockError),
}

/// Run reconcile for a journal id.
pub async fn reconcile(
    client: &Client,
    store: &Store,
    paths: &Paths,
    journal_id: &str,
    assume_not_submitted_flag: bool,
    now: Timestamp,
) -> Result<ReconcileResult, ReconcileError> {
    let identity_dir = paths.identity_dir.as_path();
    let row = get_journal(store, journal_id)?.ok_or(JournalError::NotFound)?;
    let owner = owner_status_for(identity_dir, journal_id, row.state)?;

    match row.state {
        State::Submitted | State::Matched => {
            return enrich_confirmed(client, store, paths, &row).await;
        }
        State::UploadIncomplete | State::UploadedNotSubmitted | State::Refused => {
            return Ok(ReconcileResult {
                outcome: ReconcileOutcome::Refused,
                state: row.state,
                journal_id: journal_id.to_owned(),
                owner,
                assume_available: false,
                attribution: None,
                receipt_id: None,
                posted: None,
                server_match: None,
                candidates: Vec::new(),
                message: "re-run submit".into(),
                response_kind: None,
                not_submitted_evidence: row.not_submitted_evidence.clone(),
            });
        }
        State::Planned | State::Uploading | State::Uploaded | State::Posting => {
            if matches!(owner, OwnerStatus::Live) {
                return Ok(ReconcileResult {
                    outcome: ReconcileOutcome::Recovery,
                    state: row.state,
                    journal_id: journal_id.to_owned(),
                    owner,
                    assume_available: false,
                    attribution: None,
                    receipt_id: None,
                    posted: None,
                    server_match: None,
                    candidates: Vec::new(),
                    message: "in_progress".into(),
                    response_kind: None,
                    not_submitted_evidence: None,
                });
            }
            let Some(owner_lock) = OwnerLock::try_acquire(identity_dir, journal_id)? else {
                return Ok(ReconcileResult {
                    outcome: ReconcileOutcome::Recovery,
                    state: row.state,
                    journal_id: journal_id.to_owned(),
                    owner: OwnerStatus::Live,
                    assume_available: false,
                    attribution: None,
                    receipt_id: None,
                    posted: None,
                    server_match: None,
                    candidates: Vec::new(),
                    message: "in_progress".into(),
                    response_kind: None,
                    not_submitted_evidence: None,
                });
            };
            let recovered = recover_if_owner_absent(store, identity_dir, journal_id)?;
            drop(owner_lock);
            let row = get_journal(store, journal_id)?.ok_or(JournalError::NotFound)?;
            if row.state != State::OutcomeUnknown {
                return Ok(ReconcileResult {
                    outcome: ReconcileOutcome::Recovery,
                    state: row.state,
                    journal_id: journal_id.to_owned(),
                    owner: OwnerStatus::NotApplicable,
                    assume_available: false,
                    attribution: None,
                    receipt_id: None,
                    posted: None,
                    server_match: None,
                    candidates: Vec::new(),
                    message: format!("recovered to {}", recovered.unwrap_or(row.state)),
                    response_kind: row
                        .response_kind
                        .as_deref()
                        .and_then(|s| s.parse().ok()),
                    not_submitted_evidence: row.not_submitted_evidence.clone(),
                });
            }
        }
        State::OutcomeUnknown => {}
    }

    let owner_lock = OwnerLock::try_acquire(identity_dir, journal_id)?
        .ok_or(JournalError::Lock(crate::journal::LockError::InProgress))?;
    // Re-read under owner lock.
    let row = get_journal(store, journal_id)?.ok_or(JournalError::NotFound)?;
    if row.state != State::OutcomeUnknown {
        return Ok(base_result(&row, OwnerStatus::NotApplicable, ReconcileOutcome::Recovery, "state changed"));
    }

    let history = get_submission_history(client, row.course_id, row.assignment_id).await?;
    let eval = evaluate_history(&row, &history, now)?;

    if assume_not_submitted_flag {
        if eval.newer_attempt_visible || !eval.assume_available {
            return Ok(ReconcileResult {
                outcome: ReconcileOutcome::Refused,
                state: row.state,
                journal_id: row.journal_id.clone(),
                owner: OwnerStatus::NotApplicable,
                assume_available: eval.assume_available,
                attribution: None,
                receipt_id: None,
                posted: None,
                server_match: eval.server_match.clone(),
                candidates: eval.candidates.clone(),
                message: if eval.newer_attempt_visible {
                    "cannot assume: a newer attempt is visible".into()
                } else {
                    "cannot assume: wait 30 minutes after posting_started_at".into()
                },
                response_kind: row
                    .response_kind
                    .as_deref()
                    .and_then(|s| s.parse().ok()),
                not_submitted_evidence: None,
            });
        }
        assume_not_submitted(store, &owner_lock, &row.journal_id, false, now)?;
        return Ok(ReconcileResult {
            outcome: ReconcileOutcome::Recovery,
            state: State::UploadedNotSubmitted,
            journal_id: row.journal_id.clone(),
            owner: OwnerStatus::NotApplicable,
            assume_available: false,
            attribution: None,
            receipt_id: None,
            posted: None,
            server_match: None,
            candidates: eval.candidates,
            message: "assumed not submitted; a later commit of the original request may create a second attempt".into(),
            response_kind: None,
            not_submitted_evidence: Some("assumed".into()),
        });
    }

    let result = apply_positive_evidence(store, &owner_lock, &row, &eval, paths)?;
    drop(owner_lock);
    Ok(result)
}

/// Immediate resolution (step 9b) using an already-fetched history, under an owner lock.
pub fn reconcile_history(
    store: &Store,
    owner: &OwnerLock,
    paths: &Paths,
    row: &JournalRow,
    history: &Submission,
    now: Timestamp,
) -> Result<ReconcileResult, ReconcileError> {
    let eval = evaluate_history(row, history, now)?;
    apply_positive_evidence(store, owner, row, &eval, paths)
}

struct HistoryEval {
    candidates: Vec<CandidateRecord>,
    server_match: Option<CandidateRecord>,
    file_matches: Vec<SubmissionHistoryEntry>,
    newer_attempt_visible: bool,
    assume_available: bool,
    current_attempt: i64,
}

fn evaluate_history(
    row: &JournalRow,
    history: &Submission,
    now: Timestamp,
) -> Result<HistoryEval, ReconcileError> {
    let baseline = row.baseline_attempt.unwrap_or(0);
    let started: Timestamp = row
        .posting_started_at
        .as_deref()
        .ok_or(JournalError::StateConflict)?
        .parse()
        .map_err(|_| JournalError::StateConflict)?;
    let window_start_ns = started.as_nanosecond() - WINDOW_NS;
    let current_attempt = history.attempt.as_value().copied().unwrap_or(baseline);
    let entries = history.submission_history.as_deref().unwrap_or(&[]);

    let mut candidates = Vec::new();
    let mut file_matches = Vec::new();
    let uploaded: BTreeSet<String> = {
        let ids: Vec<i64> = serde_json::from_str(&row.uploaded_file_ids_json).unwrap_or_default();
        ids.into_iter().map(|id| id.to_string()).collect()
    };
    let intent: IntendedPayload = serde_json::from_str(&row.intended_payload_json)?;
    let mut best_server_match: Option<CandidateRecord> = None;

    for entry in entries {
        let Some(attempt) = entry.attempt.as_value().copied() else {
            continue;
        };
        if attempt <= baseline {
            continue;
        }
        let submitted_at = entry.submitted_at.as_value().map(ToString::to_string);
        let submitted_ns = entry
            .submitted_at
            .as_value()
            .map(|t| t.as_nanosecond())
            .unwrap_or(i128::MIN);
        if submitted_ns < window_start_ns {
            continue;
        }
        let attachment_ids: Vec<String> = entry
            .attachments
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|a| a.id.to_string())
            .collect();
        let candidate = CandidateRecord {
            attempt,
            submitted_at: submitted_at.clone(),
            submitted_at_local: None,
            attachment_ids: attachment_ids.clone(),
        };
        candidates.push(candidate.clone());

        if row.kind == "online_upload" {
            let set: BTreeSet<_> = attachment_ids.into_iter().collect();
            if !uploaded.is_empty() && set == uploaded {
                file_matches.push(entry.clone());
            }
        } else if row.kind == "online_text_entry" {
            if let Some(text) = &intent.text
                && let Some(body) = &entry.body
                && hex_sha256(body.as_bytes()) == text.sent_sha256
                && best_server_match
                    .as_ref()
                    .is_none_or(|m| attempt > m.attempt)
            {
                best_server_match = Some(candidate);
            }
        } else if row.kind == "online_url"
            && let Some(url) = &intent.url
            && entry.url.as_ref().map(|u| u.as_str()) == Some(url.as_str())
            && best_server_match
                .as_ref()
                .is_none_or(|m| attempt > m.attempt)
        {
            best_server_match = Some(candidate);
        }
    }
    candidates.sort_by_key(|c| c.attempt);
    let server_match = best_server_match;

    let newer_attempt_visible = entries
        .iter()
        .any(|e| e.attempt.as_value().is_some_and(|a| *a > baseline))
        || current_attempt > baseline;
    let assume_available =
        !newer_attempt_visible && now.as_nanosecond() - started.as_nanosecond() >= ASSUME_NS;

    Ok(HistoryEval {
        candidates,
        server_match,
        file_matches,
        newer_attempt_visible,
        assume_available,
        current_attempt,
    })
}

fn apply_positive_evidence(
    store: &Store,
    owner: &OwnerLock,
    row: &JournalRow,
    eval: &HistoryEval,
    paths: &Paths,
) -> Result<ReconcileResult, ReconcileError> {
    let baseline = row.baseline_attempt.unwrap_or(0);

    if row.state == State::Posting {
        return Ok(unknown_result(row, eval, "posting"));
    }

    if row.kind == "online_upload" {
        match eval.file_matches.len() {
            1 => {
                let entry = &eval.file_matches[0];
                let value = history_entry_json(entry);
                let posted = allowlist_from_json(Evidence::HistoryFiles, &value, None)?;
                let readback = readback_from_entry(entry);
                let receipt = ReceiptRecord {
                    receipt_id: Uuid::new_v4().to_string(),
                    journal_id: row.journal_id.clone(),
                    attribution: "unproven".into(),
                    posted: posted.clone(),
                    readback: Some(readback),
                };
                commit_matched(store, owner, &row.journal_id, &receipt)?;
                let _ = export(store, paths, &row.journal_id, None);
                return Ok(ReconcileResult {
                    outcome: ReconcileOutcome::Ok,
                    state: State::Matched,
                    journal_id: row.journal_id.clone(),
                    owner: OwnerStatus::NotApplicable,
                    assume_available: false,
                    attribution: Some("unproven".into()),
                    receipt_id: Some(receipt.receipt_id),
                    posted: Some(posted),
                    server_match: None,
                    candidates: eval.candidates.clone(),
                    message: "matched via history file ids".into(),
                    response_kind: None,
                    not_submitted_evidence: None,
                });
            }
            0 => {
                let message = if eval.candidates.is_empty()
                    && eval.current_attempt == baseline
                    && !eval.newer_attempt_visible
                {
                    "no attempt is visible; the original request may still complete; submission reconcile re-checks; --assume-not-submitted becomes available after 30 minutes".into()
                } else {
                    format!(
                        "Canvas shows {} newer attempt(s); none match this journal's uploaded file ids",
                        eval.candidates.len()
                    )
                };
                return Ok(ReconcileResult {
                    outcome: ReconcileOutcome::Recovery,
                    state: State::OutcomeUnknown,
                    journal_id: row.journal_id.clone(),
                    owner: OwnerStatus::NotApplicable,
                    assume_available: eval.assume_available,
                    attribution: None,
                    receipt_id: None,
                    posted: None,
                    server_match: None,
                    candidates: eval.candidates.clone(),
                    message,
                    response_kind: row
                        .response_kind
                        .as_deref()
                        .and_then(|s| s.parse().ok()),
                    not_submitted_evidence: None,
                });
            }
            _ => {
                return Ok(ReconcileResult {
                    outcome: ReconcileOutcome::Recovery,
                    state: State::OutcomeUnknown,
                    journal_id: row.journal_id.clone(),
                    owner: OwnerStatus::NotApplicable,
                    assume_available: eval.assume_available,
                    attribution: None,
                    receipt_id: None,
                    posted: None,
                    server_match: None,
                    candidates: eval.candidates.clone(),
                    message: format!(
                        "multiple history entries share this journal's uploaded file ids ({})",
                        eval.file_matches.len()
                    ),
                    response_kind: row
                        .response_kind
                        .as_deref()
                        .and_then(|s| s.parse().ok()),
                    not_submitted_evidence: None,
                });
            }
        }
    }

    // Text / URL: record server_match only; stay unknown.
    if let Some(server_match) = &eval.server_match {
        let json = serde_json::to_string(&Some(server_match))?;
        transition(
            store,
            owner,
            &row.journal_id,
            State::OutcomeUnknown,
            State::OutcomeUnknown,
            TransitionPatch {
                server_match_json: Some(json),
                ..TransitionPatch::default()
            },
        )?;
        let message = format!(
            "Canvas shows matching content at attempt {}; this CLI cannot prove it created that attempt; re-running submit creates a new attempt; receipts acknowledge retires the pending flag",
            server_match.attempt
        );
        return Ok(ReconcileResult {
            outcome: ReconcileOutcome::Recovery,
            state: State::OutcomeUnknown,
            journal_id: row.journal_id.clone(),
            owner: OwnerStatus::NotApplicable,
            assume_available: eval.assume_available,
            attribution: None,
            receipt_id: None,
            posted: None,
            server_match: Some(server_match.clone()),
            candidates: eval.candidates.clone(),
            message,
            response_kind: row
                .response_kind
                .as_deref()
                .and_then(|s| s.parse().ok()),
            not_submitted_evidence: None,
        });
    }

    let message = if eval.candidates.is_empty() {
        "no attempt is visible; the original request may still complete; submission reconcile re-checks; --assume-not-submitted becomes available after 30 minutes".into()
    } else {
        format!(
            "Canvas shows {} newer attempt(s), none matching; this CLI cannot prove it created any of them",
            eval.candidates.len()
        )
    };
    Ok(ReconcileResult {
        outcome: ReconcileOutcome::Recovery,
        state: State::OutcomeUnknown,
        journal_id: row.journal_id.clone(),
        owner: OwnerStatus::NotApplicable,
        assume_available: eval.assume_available,
        attribution: None,
        receipt_id: None,
        posted: None,
        server_match: None,
        candidates: eval.candidates.clone(),
        message,
        response_kind: row
            .response_kind
            .as_deref()
            .and_then(|s| s.parse().ok()),
        not_submitted_evidence: None,
    })
}

fn unknown_result(row: &JournalRow, eval: &HistoryEval, message: &str) -> ReconcileResult {
    ReconcileResult {
        outcome: ReconcileOutcome::Recovery,
        state: row.state,
        journal_id: row.journal_id.clone(),
        owner: OwnerStatus::NotApplicable,
        assume_available: eval.assume_available,
        attribution: None,
        receipt_id: None,
        posted: None,
        server_match: eval.server_match.clone(),
        candidates: eval.candidates.clone(),
        message: message.into(),
        response_kind: row
            .response_kind
            .as_deref()
            .and_then(|s| s.parse().ok()),
        not_submitted_evidence: None,
    }
}

fn base_result(
    row: &JournalRow,
    owner: OwnerStatus,
    outcome: ReconcileOutcome,
    message: &str,
) -> ReconcileResult {
    ReconcileResult {
        outcome,
        state: row.state,
        journal_id: row.journal_id.clone(),
        owner,
        assume_available: false,
        attribution: None,
        receipt_id: None,
        posted: None,
        server_match: None,
        candidates: Vec::new(),
        message: message.into(),
        response_kind: row
            .response_kind
            .as_deref()
            .and_then(|s| s.parse().ok()),
        not_submitted_evidence: row.not_submitted_evidence.clone(),
    }
}

async fn enrich_confirmed(
    client: &Client,
    store: &Store,
    paths: &Paths,
    row: &JournalRow,
) -> Result<ReconcileResult, ReconcileError> {
    let posted: PostedRecord = serde_json::from_str(
        row.response_record_json
            .as_deref()
            .ok_or(JournalError::StateConflict)?,
    )?;
    let attempt = posted.attempt.ok_or(JournalError::StateConflict)?;
    if row.readback_record_json.is_none()
        && let Ok(owner) = OwnerLock::acquire(&paths.identity_dir, &row.journal_id)
        && let Ok(history) = get_submission_history(client, row.course_id, row.assignment_id).await
        && let Some(entry) = history
            .submission_history
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .find(|e| e.attempt.as_value().copied() == Some(attempt))
    {
        let readback = readback_from_entry(entry);
        let _ = enrich_readback(store, &owner, &row.journal_id, attempt, &readback);
        let _ = export(store, paths, &row.journal_id, None);
        drop(owner);
    }
    let row = get_journal(store, &row.journal_id)?.ok_or(JournalError::NotFound)?;
    let doc = rebuild_from_journal(store, &row.journal_id).ok();
    Ok(ReconcileResult {
        outcome: ReconcileOutcome::Ok,
        state: row.state,
        journal_id: row.journal_id.clone(),
        owner: OwnerStatus::NotApplicable,
        assume_available: false,
        attribution: doc.as_ref().map(|d| d.attribution.clone()),
        receipt_id: doc.as_ref().map(|d| d.receipt_id.clone()),
        posted: Some(posted),
        server_match: None,
        candidates: Vec::new(),
        message: "confirmed".into(),
        response_kind: None,
        not_submitted_evidence: None,
    })
}

pub(crate) fn history_entry_json(entry: &SubmissionHistoryEntry) -> Value {
    let mut map = serde_json::Map::new();
    if let Some(id) = entry.id {
        map.insert("id".into(), json_num_or_str(id));
    }
    if let Some(attempt) = entry.attempt.as_value() {
        map.insert("attempt".into(), (*attempt).into());
    }
    if let Some(submitted_at) = entry.submitted_at.as_value() {
        map.insert("submitted_at".into(), submitted_at.to_string().into());
    }
    if let Some(ws) = entry.workflow_state.as_value() {
        map.insert("workflow_state".into(), ws.clone().into());
    }
    if let Some(late) = entry.late.as_value() {
        map.insert("late".into(), (*late).into());
    }
    if let Some(missing) = entry.missing.as_value() {
        map.insert("missing".into(), (*missing).into());
    }
    if let Some(excused) = entry.excused.as_value() {
        map.insert("excused".into(), (*excused).into());
    }
    if let Some(st) = &entry.submission_type {
        map.insert("submission_type".into(), st.clone().into());
    }
    if let Some(body) = &entry.body {
        map.insert("body".into(), body.clone().into());
    }
    if let Some(url) = &entry.url {
        map.insert("url".into(), url.as_str().into());
    }
    if let Some(attachments) = &entry.attachments {
        let arr: Vec<Value> = attachments
            .iter()
            .map(|a| {
                serde_json::json!({
                    "id": a.id,
                    "display_name": a.display_name,
                    "size": a.size,
                    "content_type": a.content_type,
                })
            })
            .collect();
        map.insert("attachments".into(), Value::Array(arr));
    }
    Value::Object(map)
}

fn json_num_or_str(id: i64) -> Value {
    id.into()
}

pub(crate) fn readback_from_entry(entry: &SubmissionHistoryEntry) -> ReadbackRecord {
    let body_sha256 = entry.body.as_ref().map(|b| hex_sha256(b.as_bytes()));
    let attachments = entry
        .attachments
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|a| crate::journal::AttachmentRecord {
            id: a.id.to_string(),
            display_name: a.display_name.clone(),
            size: a.size,
            content_type: a.content_type.clone(),
        })
        .collect();
    ReadbackRecord {
        submitted_at: entry.submitted_at.as_value().map(ToString::to_string),
        submitted_at_local: None,
        late: entry.late.as_value().copied(),
        attachments,
        body_sha256,
    }
}

pub(crate) fn submission_value_from_bytes(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice(bytes).ok()
}
