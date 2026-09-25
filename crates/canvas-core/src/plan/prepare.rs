//! `prepare`: freeze one remote write and store it for approval.
//!
//! Preparing is an authorized local organization step: it reads the assignment,
//! freezes the exact bytes, and writes one `prepared` row. It never uploads,
//! never posts, and never holds the admission lock across human consideration.

use std::path::Path;

use canvas_api::Client;
use jiff::Timestamp;

use crate::store::Store;
use crate::submit::{FreezeError, FrozenInput, InputKind, Plan, PreflightError};

use super::PlanError;
use super::ops::{self, NewPlan};
use super::record::{Observations, PlanKind, PlanRow};

/// What [`prepare`] needs besides the client, the store, and the input.
#[derive(Debug, Clone, Copy)]
pub struct PrepareRequest<'a> {
    /// Identity directory holding the lock and journal files.
    pub identity_dir: &'a Path,
    /// Identity key the plan is bound to.
    pub identity_key: &'a str,
    /// Consumer that asked for the plan, when one did.
    pub consumer: Option<&'a str>,
    /// Course id.
    pub course_id: i64,
    /// Assignment id.
    pub assignment_id: i64,
    /// Course code to use when the assignment `GET` omits the course include.
    pub course_code: Option<&'a str>,
    /// Submission kind the caller intends to freeze.
    pub kind: InputKind,
}

/// A stored plan and the confirmation view of it.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// The stored `prepared` row.
    pub plan: PlanRow,
    /// The same plan in the shape `canvas submit` prints and confirms.
    pub display: Plan,
}

/// Freeze a submission and store it as a `prepared` plan.
///
/// The order is SPEC §12.2 pre-flight: step 1 fresh `GET`, step 2 admission and
/// owner-absent recovery, steps 3–4 eligibility, step 5 freeze, step 6 baseline.
/// The admission lock covers that pre-flight and is released before this
/// returns, so nothing is held while a person decides.
pub async fn prepare<F>(
    client: &Client,
    store: &Store,
    request: &PrepareRequest<'_>,
    freeze: F,
    now: Timestamp,
) -> Result<Prepared, PlanError>
where
    F: FnOnce() -> Result<FrozenInput, FreezeError> + Send + 'static,
{
    let kind = request.kind;
    // Step 1.
    let assignment =
        crate::submit::fetch_assignment(client, request.course_id, request.assignment_id).await?;

    // Step 2.
    let admission = crate::submit::admit(request.identity_dir, request.assignment_id)?;
    let recovered =
        crate::submit::recover_active(store, request.identity_dir, request.assignment_id)?;

    // Steps 3–4, then step 5 under the same admission.
    crate::submit::check_admissible(&assignment, kind, now)?;
    let mut frozen = crate::submit::freeze_on_worker(kind, freeze).await?;
    crate::submit::check_extensions(&assignment, &frozen)?;

    // Step 6.
    let mut facts = crate::submit::facts_of(&assignment, now);
    if facts.course_code.is_none() {
        facts.course_code = request.course_code.map(str::to_owned);
    }
    crate::submit::enrich_payload(store, &mut frozen, &facts)?;

    // The pre-flight is over; nothing is held while the plan waits.
    drop(admission);

    let file_paths = frozen
        .file_paths
        .iter()
        .map(|path| {
            path.to_str().map(str::to_owned).ok_or_else(|| {
                PlanError::Preflight(PreflightError::Validation(
                    "file path is not valid UTF-8".into(),
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let plan = ops::insert(
        store,
        NewPlan {
            identity_key: request.identity_key.to_owned(),
            identity_generation: ops::identity_generation(store)?,
            consumer: request.consumer.map(str::to_owned),
            course_id: request.course_id,
            assignment_id: request.assignment_id,
            kind: PlanKind::Submission(frozen.kind),
            payload: frozen.payload.clone(),
            file_paths,
            baseline_attempt: facts.baseline_attempt,
            baseline_submission_id: facts.baseline_submission_id,
            observations: Observations::of(&assignment),
            operation: None,
        },
        now,
    )?;

    let display = crate::submit::plan_of(
        request.course_id,
        request.assignment_id,
        &facts,
        frozen,
        recovered,
    );
    Ok(Prepared { plan, display })
}
