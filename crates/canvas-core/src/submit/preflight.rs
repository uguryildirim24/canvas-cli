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
#[allow(clippy::too_many_arguments)]
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
    preflight_with_input(
        client,
        store,
        identity_dir,
        identity_key,
        course_id,
        assignment_id,
        frozen.kind,
        move || Ok(frozen),
        now,
    )
    .await
}

/// Assignment facts pre-flight derives for the plan (steps 1 and 6).
///
/// The plan layer needs the same numbers the confirmation prints, so they are
/// derived once here and shared by `submit` and `canvas-core::plan`.
#[derive(Debug, Clone, Default)]
pub struct AssignmentFacts {
    /// Baseline attempt (step 6).
    pub baseline_attempt: i64,
    /// Baseline submission id (step 6).
    pub baseline_submission_id: Option<i64>,
    /// Due-at when known.
    pub due_at: Option<String>,
    /// Past due; a warning, never a refusal.
    pub past_due: bool,
    /// Assignment name when known.
    pub assignment_name: Option<String>,
    /// Course code when known.
    pub course_code: Option<String>,
}

/// Step 1: fresh `GET` with `include[]=submission&include[]=can_submit`.
pub async fn fetch_assignment(
    client: &Client,
    course_id: i64,
    assignment_id: i64,
) -> Result<Assignment, PreflightError> {
    Ok(get_assignment_for_submit(client, course_id, assignment_id).await?)
}

/// Step 2: take the admission lock for one assignment.
pub fn admit(identity_dir: &Path, assignment_id: i64) -> Result<AdmissionLock, PreflightError> {
    AdmissionLock::try_acquire(identity_dir, assignment_id).map_err(|e| match e {
        LockError::InProgress => PreflightError::InProgress { journal_id: None },
        LockError::Io(err) => PreflightError::Io(err),
    })
}

/// Steps 3 and 4: group and submission-type rules, then eligibility.
pub fn check_admissible(
    assignment: &Assignment,
    kind: InputKind,
    now: Timestamp,
) -> Result<(), PreflightError> {
    check_group_and_types(assignment, kind)?;
    check_eligibility(assignment, now)
}

/// Step 5 tail: every uploaded name must carry an allowed extension.
pub fn check_extensions(
    assignment: &Assignment,
    frozen: &FrozenInput,
) -> Result<(), PreflightError> {
    if frozen.kind != InputKind::OnlineUpload {
        return Ok(());
    }
    let Some(extensions) = assignment
        .allowed_extensions
        .as_value()
        .filter(|xs| !xs.is_empty())
    else {
        return Ok(());
    };
    for file in &frozen.payload.files {
        let extension = Path::new(&file.name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if !extensions
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(extension))
        {
            return Err(PreflightError::Refused(format!(
                "disallowed extension: {}",
                file.name
            )));
        }
    }
    Ok(())
}

/// Step 6 baseline, plus the names the confirmation prints.
#[must_use]
pub fn facts_of(assignment: &Assignment, now: Timestamp) -> AssignmentFacts {
    AssignmentFacts {
        baseline_attempt: assignment
            .submission
            .as_ref()
            .and_then(|s| s.attempt.as_value().copied())
            .unwrap_or(0),
        baseline_submission_id: assignment.submission.as_ref().and_then(|s| s.id),
        due_at: assignment.due_at.as_value().map(ToString::to_string),
        past_due: assignment.due_at.as_value().is_some_and(|due| *due < now),
        assignment_name: assignment.name.as_value().cloned(),
        course_code: assignment
            .course
            .as_ref()
            .and_then(|c| c.course_code.clone()),
    }
}

/// Copy the identity time zone and the assignment names into a frozen payload.
pub fn enrich_payload(
    store: &Store,
    frozen: &mut FrozenInput,
    facts: &AssignmentFacts,
) -> Result<(), PreflightError> {
    frozen.payload.time_zone = store
        .call_blocking(|c| {
            use rusqlite::OptionalExtension;
            Ok(c.state
                .query_row(
                    "SELECT value FROM identity WHERE key='time_zone'",
                    [],
                    |r| r.get(0),
                )
                .optional()?)
        })
        .map_err(JournalError::from)?;
    frozen
        .payload
        .assignment_name
        .clone_from(&facts.assignment_name);
    frozen.payload.course_code.clone_from(&facts.course_code);
    frozen.payload.due_at.clone_from(&facts.due_at);
    Ok(())
}

/// Run the ordered preflight, freezing on a blocking worker only after eligibility.
#[allow(clippy::too_many_arguments)]
pub async fn preflight_with_input<F>(
    client: &Client,
    store: &Store,
    identity_dir: &Path,
    identity_key: &str,
    course_id: i64,
    assignment_id: i64,
    kind: InputKind,
    freeze: F,
    now: Timestamp,
) -> Result<PreflightOutcome, PreflightError>
where
    F: FnOnce() -> Result<FrozenInput, FreezeError> + Send + 'static,
{
    let _ = identity_key;
    // Step 1: fresh assignment GET.
    let assignment = fetch_assignment(client, course_id, assignment_id).await?;

    // Step 2: admission lock + owner-absent recovery.
    let admission = admit(identity_dir, assignment_id)?;
    let recovered = recover_active(store, identity_dir, assignment_id)?;

    // Steps 3–4.
    check_admissible(&assignment, kind, now)?;

    // Step 5: freeze once under admission, after eligibility.
    let mut frozen = freeze_on_worker(kind, freeze).await?;
    check_extensions(&assignment, &frozen)?;

    // Step 6: baseline from the fresh assignment.
    let facts = facts_of(&assignment, now);
    enrich_payload(store, &mut frozen, &facts)?;

    Ok(PreflightOutcome {
        plan: plan_of(course_id, assignment_id, &facts, frozen, recovered),
        admission,
    })
}

/// Step 5: run the freeze closure on a blocking worker and confirm the kind.
pub(crate) async fn freeze_on_worker<F>(
    kind: InputKind,
    freeze: F,
) -> Result<FrozenInput, PreflightError>
where
    F: FnOnce() -> Result<FrozenInput, FreezeError> + Send + 'static,
{
    let frozen = tokio::task::spawn_blocking(freeze)
        .await
        .map_err(|_| std::io::Error::other("input worker failed"))??;
    if frozen.kind != kind {
        return Err(PreflightError::Validation("input kind changed".into()));
    }
    Ok(frozen)
}

/// Assemble the confirmation plan from the frozen input and the fresh facts.
pub(crate) fn plan_of(
    course_id: i64,
    assignment_id: i64,
    facts: &AssignmentFacts,
    frozen: FrozenInput,
    recovered: Vec<(String, State)>,
) -> Plan {
    Plan {
        course_id,
        assignment_id,
        assignment_name: facts.assignment_name.clone(),
        course_code: facts.course_code.clone(),
        due_at: facts.due_at.clone(),
        past_due: facts.past_due,
        kind: frozen.kind,
        estimated_attempt: facts.baseline_attempt + 1,
        baseline_attempt: facts.baseline_attempt,
        baseline_submission_id: facts.baseline_submission_id,
        frozen,
        recovered,
    }
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

/// Step 2 recovery: adopt or refuse every active journal for this assignment.
pub fn recover_active(
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
    store
        .call_blocking(move |conns| {
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
        return Err(PreflightError::Refused(
            "assignment lock_at is in the past".into(),
        ));
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
        if i128::from(used) >= i128::from(allowed) + i128::from(extra) {
            return Err(PreflightError::Refused("no attempts remaining".into()));
        }
    }
    Ok(())
}
