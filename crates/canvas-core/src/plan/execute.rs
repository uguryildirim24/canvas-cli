//! `execute`: revalidate an approved plan and admit it to a journal.
//!
//! Execute is the only door from a plan to a journal. It refuses an expired,
//! invalidated, or unapproved plan before any network write, revalidates every
//! fact the plan froze, and then runs one state transaction that consumes the
//! approval, creates the journal, and marks the plan `executed`. Uploads begin
//! only after that transaction commits; from there SPEC §12.2 is unchanged.

use std::path::{Path, PathBuf};

use canvas_api::Client;
use jiff::Timestamp;

use crate::journal::{AdmissionLock, CreateOpts, JournalError, OwnerLock, PlanLink, create_linked};
use crate::store::Store;
use crate::submit::{FrozenInput, InputKind, freeze_files};

use super::ops;
use super::record::{Observations, PlanRow, PlanState};
use super::{HandleRefusal, PlanError};

/// What one execute produced.
#[derive(Debug)]
pub enum Admission {
    /// A new journal. This process owns it and runs §12.2 steps 8–12.
    Created {
        /// Journal id.
        journal_id: String,
        /// Owner lock, held until the journal is terminal.
        owner: OwnerLock,
        /// The frozen input, rebuilt from the plan.
        frozen: Box<FrozenInput>,
        /// Past-due warning from the revalidation read.
        past_due: bool,
    },
    /// This plan already admitted a journal; no second attempt was created.
    Existing {
        /// The journal the plan admitted.
        journal_id: String,
    },
}

/// The local half of execute, without the network revalidation.
pub(super) enum Linked {
    /// A new journal and its owner lock.
    Created(String, OwnerLock),
    /// The journal this plan had already admitted.
    Existing(String),
}

/// Revalidate an approved plan and admit it to a journal.
pub async fn execute(
    client: &Client,
    store: &Store,
    identity_dir: &Path,
    identity_key: &str,
    plan_id: &str,
    now: Timestamp,
) -> Result<Admission, PlanError> {
    let plan = ops::require(store, plan_id)?;

    // A concurrent execute, a restarted host, or a replayed approval gets the
    // journal this plan already admitted. Expiry gates first admission only, so
    // it is never evaluated for a plan that is already executed.
    if plan.state == PlanState::Executed {
        return plan
            .journal_id
            .clone()
            .map(|journal_id| Admission::Existing { journal_id })
            .ok_or(PlanError::Refused {
                reason: "invalidated",
                message: "executed plan has no journal".into(),
            });
    }

    ops::guard_admission(&plan, now)?;
    if plan.state != PlanState::Approved || plan.approval.is_none() {
        return Err(PlanError::Refused {
            reason: "approval_required",
            message: "plan has no recorded human approval".into(),
        });
    }
    // The stored plan must still be the plan that was approved.
    if plan.digest() != plan.plan_sha256 {
        return Err(PlanError::Handle(HandleRefusal::DigestMismatch));
    }
    if plan.identity_key != identity_key {
        return Err(invalidate(store, &plan, "plan belongs to another identity"));
    }
    if ops::identity_generation(store)? != plan.identity_generation {
        return Err(invalidate(store, &plan, "identity generation changed"));
    }

    // Pre-flight step 1 again: the plan is compared against a fresh read.
    let assignment =
        crate::submit::fetch_assignment(client, plan.course_id, plan.assignment_id).await?;

    // Step 2: admission for the whole of the rest of this function. When another
    // process holds it, that is usually this plan's own concurrent execute, so
    // wait briefly for its journal rather than reporting a conflict.
    let admission = match crate::submit::admit(identity_dir, plan.assignment_id) {
        Ok(lock) => lock,
        Err(crate::submit::PreflightError::InProgress { journal_id }) => {
            return wait_for_existing(store, &plan, journal_id).await;
        }
        Err(other) => return Err(other.into()),
    };
    // Admission serializes plans for this assignment, so the first read under
    // it is authoritative: a concurrent execute may have finished in between.
    let plan = ops::require(store, plan_id)?;
    if plan.state == PlanState::Executed {
        drop(admission);
        return plan
            .journal_id
            .map(|journal_id| Admission::Existing { journal_id })
            .ok_or(PlanError::Refused {
                reason: "invalidated",
                message: "executed plan has no journal".into(),
            });
    }
    // The first read happened before the revalidation `GET`; a decline, a
    // cancel, or an expiry could have landed while it was in flight. Admission
    // makes this read authoritative, so the refusal names what actually
    // happened instead of letting the guarded link report a lost race.
    ops::guard_admission(&plan, now)?;
    if plan.state != PlanState::Approved || plan.approval.is_none() {
        return Err(PlanError::Refused {
            reason: "approval_required",
            message: "plan has no recorded human approval".into(),
        });
    }
    crate::submit::recover_active(store, identity_dir, plan.assignment_id)?;

    // Every observation the plan froze, then steps 3–4 on the fresh read. The
    // comparison comes first so a fact that both changed and now refuses is
    // reported as the changed fact: it needs a fresh plan, not a retry.
    let fresh = Observations::of(&assignment);
    if let Some(field) = plan.observations.first_difference(&fresh) {
        return Err(invalidate(
            store,
            &plan,
            &format!("{field} changed since the plan was prepared"),
        ));
    }
    crate::submit::check_admissible(&assignment, plan.kind, now)?;

    // Step 6: the baseline the plan promised must still be the baseline.
    let facts = crate::submit::facts_of(&assignment, now);
    if facts.baseline_attempt != plan.baseline_attempt
        || facts.baseline_submission_id != plan.baseline_submission_id
    {
        return Err(invalidate(
            store,
            &plan,
            "baseline attempt changed since the plan was prepared",
        ));
    }

    let frozen = frozen_of(&plan);
    crate::submit::check_extensions(&assignment, &frozen)?;
    verify_local_bytes(store, &plan)?;

    let linked = link(store, identity_dir, &admission, &plan)?;
    drop(admission);
    Ok(match linked {
        Linked::Created(journal_id, owner) => Admission::Created {
            journal_id,
            owner,
            frozen: Box::new(frozen),
            past_due: facts.past_due,
        },
        Linked::Existing(journal_id) => Admission::Existing { journal_id },
    })
}

/// How long a blocked execute waits for its own concurrent execute to commit.
const CONTENTION_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Wait for a concurrent execute of this plan, then return its journal.
///
/// REPORT §3.5: a concurrent execute, a restarted host, or a replayed approval
/// returns the existing journal and never creates a second attempt. Admission
/// held by anything else is still the ordinary in-progress refusal.
async fn wait_for_existing(
    store: &Store,
    plan: &PlanRow,
    journal_id: Option<String>,
) -> Result<Admission, PlanError> {
    let deadline = std::time::Instant::now() + CONTENTION_WAIT;
    loop {
        if let Some(current) = ops::load(store, &plan.plan_id)?
            && current.state == PlanState::Executed
            && let Some(journal_id) = current.journal_id
        {
            return Ok(Admission::Existing { journal_id });
        }
        if std::time::Instant::now() >= deadline {
            return Err(PlanError::InProgress { journal_id });
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

/// The one state transaction: consume the approval and publish the journal.
///
/// This is separate from [`execute`] so the crash tests can kill a process at
/// the transaction boundary without a network in the way.
pub(super) fn link(
    store: &Store,
    identity_dir: &Path,
    admission: &AdmissionLock,
    plan: &PlanRow,
) -> Result<Linked, PlanError> {
    let approval = plan.approval.as_ref().ok_or(PlanError::Refused {
        reason: "approval_required",
        message: "plan has no recorded human approval".into(),
    })?;
    let opts = CreateOpts {
        identity_key: plan.identity_key.clone(),
        course_id: plan.course_id,
        assignment_id: plan.assignment_id,
        kind: plan.kind.as_str().to_owned(),
        intended_payload_json: serde_json::to_string(&plan.payload)?,
        baseline_attempt: Some(plan.baseline_attempt),
        baseline_submission_id: plan.baseline_submission_id,
    };
    let link = PlanLink {
        plan_id: plan.plan_id.clone(),
        approval_json: serde_json::to_string(approval)?,
    };
    match create_linked(store, identity_dir, admission, &opts, Some(&link)) {
        Ok((journal_id, owner)) => Ok(Linked::Created(journal_id, owner)),
        Err(JournalError::InProgress) => Err(PlanError::InProgress { journal_id: None }),
        // The guarded plan update matched zero rows: another execute consumed
        // the approval first, so its journal is the answer.
        Err(JournalError::StateConflict) => match ops::require(store, &plan.plan_id)?.journal_id {
            Some(journal_id) => Ok(Linked::Existing(journal_id)),
            None => Err(PlanError::Refused {
                reason: "approval_required",
                message: "the approval was consumed by another execute".into(),
            }),
        },
        Err(other) => Err(PlanError::Journal(other)),
    }
}

/// Rebuild the frozen input the plan stored.
pub(super) fn frozen_of(plan: &PlanRow) -> FrozenInput {
    FrozenInput {
        kind: plan.kind,
        payload: plan.payload.clone(),
        file_paths: plan.file_paths.iter().map(PathBuf::from).collect(),
    }
}

/// Re-read the local files and confirm they still hash to the approved digests.
///
/// Text and URL plans carry their outbound bytes, so nothing on disk can change
/// what they send. Uploads carry only a name, a size, and a digest, so the
/// bytes behind them are read again here. §12.2 step 8 still verifies the
/// streamed hash; this check refuses the change before a journal exists.
fn verify_local_bytes(store: &Store, plan: &PlanRow) -> Result<(), PlanError> {
    if plan.kind != InputKind::OnlineUpload {
        return Ok(());
    }
    if plan.file_paths.len() != plan.payload.files.len() {
        return Err(invalidate(store, plan, "frozen file list is unreadable"));
    }
    let paths: Vec<PathBuf> = plan.file_paths.iter().map(PathBuf::from).collect();
    let fresh = match freeze_files(&paths, plan.payload.comment.as_deref()) {
        Ok(fresh) => fresh,
        Err(e) => {
            return Err(invalidate(
                store,
                plan,
                &format!("frozen input is no longer readable: {e}"),
            ));
        }
    };
    for (approved, current) in plan.payload.files.iter().zip(&fresh.payload.files) {
        if approved.name != current.name
            || approved.size != current.size
            || approved.sha256 != current.sha256
        {
            return Err(invalidate(
                store,
                plan,
                &format!("{} changed since the plan was prepared", approved.name),
            ));
        }
    }
    Ok(())
}

/// Mark the plan invalidated and return the refusal that says why.
fn invalidate(store: &Store, plan: &PlanRow, reason: &str) -> PlanError {
    if let Err(e) = ops::invalidate(store, &plan.plan_id, reason) {
        return e;
    }
    PlanError::Invalidated {
        reason: reason.to_owned(),
    }
}
