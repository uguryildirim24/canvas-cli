//! Preflight steps 1–7 (§12.2).

use std::path::Path;

use canvas_api::models::Assignment;
use canvas_api::serde_util::Supplied;
use canvas_api::{Client, get_assignment_for_submit};
use jiff::Timestamp;
use thiserror::Error;

use crate::journal::{
    AdmissionLock, CreateOpts, JournalError, LockError, OwnerLock, State, create, get_journal,
    recover_if_owner_absent,
};
use crate::store::Store;
use crate::submit::freeze::{FreezeError, FrozenInput, InputKind};

/// Preflight failures before or during plan creation.
#[derive(Debug, Error)]
pub enum PreflightError {
    /// Network failure before any journal (exit 4).
    #[error(transparent)]
    Network(#[from] canvas_api::Error),
    /// Another submit is in progress (exit 8).
    #[error("in_progress")]
    InProgress { journal_id: Option<String> },
    /// Assignment / eligibility refusal (exit 8).
    #[error("{0}")]
    Refused(String),
    /// Input validation (exit 2).
    #[error("{0}")]
    Validation(String),
    /// Freeze error.
    #[error(transparent)]
    Freeze(#[from] FreezeError),
    /// Journal error.
    #[error(transparent)]
    Journal(#[from] JournalError),
    /// I/O.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Lock error.
    #[error(transparent)]
    Lock(#[from] LockError),
}

/// Plan printed for confirmation (step 7).
#[derive(Debug, Clone)]
pub struct Plan {
    /// Course id.
    pub course_id: i64,
    /// Assignment id.
    pub assignment_id: i64,
    /// Assignment name when known.
    pub assignment_name: Option<String>,
    /// Course code when known.
    pub course_code: Option<String>,
    /// Due-at when known.
    pub due_at: Option<String>,
    /// Past due warning (does not refuse).
    pub past_due: bool,
    /// Submission kind.
    pub kind: InputKind,
    /// Estimated attempt = baseline + 1.
    pub estimated_attempt: i64,
    /// Baseline attempt.
    pub baseline_attempt: i64,
    /// Baseline submission id.
    pub baseline_submission_id: Option<i64>,
    /// Frozen input.
    pub frozen: FrozenInput,
    /// Recovered journal states printed during admission (if any).
    pub recovered: Vec<(String, State)>,
}

/// Result of preflight: caller still holds the admission lock.
pub struct PreflightOutcome {
    /// Plan for confirmation.
    pub plan: Plan,
    /// Admission lock (release after [`create_from_plan`]).
    pub admission: AdmissionLock,
}

/// Run preflight steps 1–6 and build the plan. Does not create the journal.
///
/// Caller must confirm, then call [`create_from_plan`], then drop `admission`.
pub async fn preflight(
    client: &Client,
    store: &Store,
    identity_dir: &Path,
    identity_key: &str,
    course_id: i64,
    assignment_id: i64,
    frozen: FrozenInput,
    now: Timestamp,
) -> Result<PreflightOutcome, PreflightError> {
    let _ = identity_key;
    // Step 1: fresh assignment GET.
    let assignment = get_assignment_for_submit(client, course_id, assignment_id).await?;

    // Step 2: admission lock + owner-absent recovery.
    let admission = AdmissionLock::try_acquire(identity_dir, assignment_id).map_err(|e| {
        match e {
            LockError::InProgress => PreflightError::InProgress { journal_id: None },
            LockError::Io(err) => PreflightError::Io(err),
        }
    })?;
    let recovered = recover_active(store, identity_dir, assignment_id)?;

    // Steps 3–4.
    check_group_and_types(&assignment, frozen.kind)?;
    check_eligibility(&assignment, now)?;

    // Steps 5–6 (freeze already done by caller; baseline from assignment).
    let baseline_attempt = assignment
        .submission
        .as_ref()
        .and_then(|s| s.attempt.as_value().copied())
        .unwrap_or(0);
    let baseline_submission_id = assignment.submission.as_ref().and_then(|s| s.id);
    let due_at = assignment
        .due_at
        .as_value()
        .map(ToString::to_string);
    let past_due = assignment
        .due_at
        .as_value()
        .is_some_and(|due| *due < now);
    let assignment_name = assignment.name.as_value().cloned();
    let course_code = assignment
        .course
        .as_ref()
        .and_then(|c| c.course_code.clone());

    let mut frozen = frozen;
    frozen.payload.assignment_name = assignment_name.clone();
    frozen.payload.course_code = course_code.clone();
    frozen.payload.due_at = due_at.clone();

    Ok(PreflightOutcome {
        plan: Plan {
            course_id,
            assignment_id,
            assignment_name,
            course_code,
            due_at,
            past_due,
            kind: frozen.kind,
            estimated_attempt: baseline_attempt + 1,
            baseline_attempt,
            baseline_submission_id,
            frozen,
            recovered,
        },
        admission,
    })
}

/// Step 7: create the journal under the held admission lock; caller releases admission after.
pub fn create_from_plan(
    store: &Store,
    identity_dir: &Path,
    identity_key: &str,
    admission: &AdmissionLock,
    plan: &Plan,
) -> Result<(String, OwnerLock), PreflightError> {
    let opts = CreateOpts {
        identity_key: identity_key.to_owned(),
        course_id: plan.course_id,
        assignment_id: plan.assignment_id,
        kind: plan.kind.as_str().to_owned(),
        intended_payload_json: serde_json::to_string(&plan.frozen.payload)?,
        baseline_attempt: Some(plan.baseline_attempt),
        baseline_submission_id: plan.baseline_submission_id,
    };
    create(store, identity_dir, admission, &opts).map_err(|e| match e {
        JournalError::InProgress => PreflightError::InProgress { journal_id: None },
        other => PreflightError::Journal(other),
    })
}

fn recover_active(
    store: &Store,
    identity_dir: &Path,
    assignment_id: i64,
) -> Result<Vec<(String, State)>, PreflightError> {
    let ids = active_journal_ids(store, assignment_id)?;
    let mut recovered = Vec::new();
    for jid in ids {
        let row = get_journal(store, &jid)?.ok_or(JournalError::NotFound)?;
        match crate::journal::owner_status_for(identity_dir, &jid, row.state)? {
            crate::journal::OwnerStatus::Live => {
                return Err(PreflightError::InProgress {
                    journal_id: Some(jid),
                });
            }
            crate::journal::OwnerStatus::Absent | crate::journal::OwnerStatus::NotApplicable => {
                if let Some(state) = recover_if_owner_absent(store, identity_dir, &jid)? {
                    recovered.push((jid, state));
                }
            }
        }
    }
    Ok(recovered)
}

fn active_journal_ids(store: &Store, assignment_id: i64) -> Result<Vec<String>, JournalError> {
    store.call_blocking(move |conns| {
        let mut stmt = conns.state.prepare(
            "SELECT journal_id FROM submission_journal
             WHERE assignment_id = ?1
               AND state IN ('planned','uploading','uploaded','posting')",
        )?;
        let rows = stmt.query_map([assignment_id], |r| r.get(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    })
    .map_err(JournalError::from)
}

fn check_group_and_types(assignment: &Assignment, kind: InputKind) -> Result<(), PreflightError> {
    if assignment.group_category_id.is_some() {
        return Err(PreflightError::Refused(
            "group submissions are not supported in v1".into(),
        ));
    }
    let Some(types) = assignment.submission_types.as_value() else {
        return Err(PreflightError::Refused(
            "assignment submission types are unknown".into(),
        ));
    };
    if types.len() == 1 && types[0] == "external_tool" {
        return Err(PreflightError::Refused(
            "external_tool assignments cannot be submitted via the CLI".into(),
        ));
    }
    let needed = kind.as_str();
    if !types.iter().any(|t| t == needed) {
        return Err(PreflightError::Refused(format!(
            "assignment does not accept {needed}"
        )));
    }
    Ok(())
}

fn check_eligibility(assignment: &Assignment, now: Timestamp) -> Result<(), PreflightError> {
    match &assignment.can_submit {
        Supplied::Value(false) => {
            let reason = assignment
                .lock_explanation
                .clone()
                .unwrap_or_else(|| "can_submit is false".into());
            return Err(PreflightError::Refused(reason));
        }
        Supplied::Value(true) => return Ok(()),
        Supplied::Absent | Supplied::Null => {}
    }
    if assignment.locked_for_user == Some(true) {
        return Err(PreflightError::Refused(
            assignment
                .lock_explanation
                .clone()
                .unwrap_or_else(|| "assignment is locked".into()),
        ));
    }
    if let Some(lock_at) = assignment.lock_at.as_value()
        && *lock_at < now
    {
        return Err(PreflightError::Refused("assignment lock_at is in the past".into()));
    }
    if let Some(unlock_at) = assignment.unlock_at.as_value()
        && *unlock_at > now
    {
        return Err(PreflightError::Refused(
            "assignment unlock_at is in the future".into(),
        ));
    }
    let allowed = assignment.allowed_attempts.as_value().copied();
    if let Some(allowed) = allowed
        && allowed != -1
    {
        let used = assignment
            .submission
            .as_ref()
            .and_then(|s| s.attempt.as_value().copied())
            .unwrap_or(0);
        let extra = assignment
            .submission
            .as_ref()
            .and_then(|s| s.extra_attempts)
            .unwrap_or(0);
        if used >= allowed + extra {
            return Err(PreflightError::Refused(
                "no attempts remaining".into(),
            ));
        }
    }
    Ok(())
}
