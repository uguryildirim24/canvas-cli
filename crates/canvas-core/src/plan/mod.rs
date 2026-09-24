//! Operation plans and approval.
//!
//! A plan is the frozen description of one remote write, held between the
//! moment the content is fixed and the moment a person approves it. It exists
//! so that approval can name exact bytes rather than an intention:
//!
//! ```text
//! prepare  →  issue_handle  →  approve  →  execute  →  journal (SPEC §12.2)
//! ```
//!
//! Three properties carry the design:
//!
//! - **Preparing costs nothing and blocks nothing.** It runs pre-flight step 1,
//!   takes the admission lock for its own pre-flight only, and releases it
//!   before returning. No lock is held while a person considers the plan.
//! - **Approval is bound.** A random server-issued handle names one plan, one
//!   consumer, and one deadline; the stored audit names the channel, the time,
//!   and the digest of the exact plan approved.
//! - **Execute is the only door to a journal.** It reacquires admission,
//!   revalidates every fact the plan froze, and links the plan to a journal in
//!   one transaction guarded by a unique index. From the journal insert onward
//!   SPEC §12.2 is unchanged.

mod execute;
mod ops;
mod prepare;
mod record;

pub use execute::{Admission, execute};
pub use ops::{
    Awaiting, EXPIRY, NewOperationPlan, NewPlan, approve, awaiting_decision, cancel, decline,
    expire, guard_admission, identity_generation, insert, insert_operation, invalidate,
    issue_handle, load, refusal_for, require,
};
pub use prepare::{PrepareRequest, Prepared, prepare};
pub use record::{
    Approval, ApprovalChannel, Observations, PlanKind, PlanRow, PlanState, sha256_hex,
};

use thiserror::Error;

use crate::journal::JournalError;
use crate::store::DbError;
use crate::submit::PreflightError;

#[cfg(test)]
mod crash_tests;
#[cfg(test)]
mod tests;

/// Why an approval handle was rejected.
///
/// Each variant is a separate refusal so a caller can say what went wrong
/// without ever revealing whether some other handle would have worked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleRefusal {
    /// No such handle.
    Unknown,
    /// The handle names a different plan.
    OtherPlan,
    /// The handle was already spent.
    AlreadyUsed,
    /// The handle belongs to another consumer.
    WrongConsumer,
    /// The handle outlived its deadline.
    Expired,
    /// The stored plan no longer matches its digest.
    DigestMismatch,
}

impl HandleRefusal {
    /// Wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown_handle",
            Self::OtherPlan => "handle_for_another_plan",
            Self::AlreadyUsed => "handle_already_used",
            Self::WrongConsumer => "wrong_consumer",
            Self::Expired => "handle_expired",
            Self::DigestMismatch => "plan_digest_mismatch",
        }
    }

    /// Parse a wire name.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "unknown_handle" => Some(Self::Unknown),
            "handle_for_another_plan" => Some(Self::OtherPlan),
            "handle_already_used" => Some(Self::AlreadyUsed),
            "wrong_consumer" => Some(Self::WrongConsumer),
            "handle_expired" => Some(Self::Expired),
            "plan_digest_mismatch" => Some(Self::DigestMismatch),
            _ => None,
        }
    }
}

impl std::fmt::Display for HandleRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Plan-layer failures.
#[derive(Debug, Error)]
pub enum PlanError {
    /// No such plan.
    #[error("plan not found")]
    NotFound,
    /// The plan cannot be admitted (exit 8).
    #[error("{message}")]
    Refused {
        /// `expired`, `invalidated`, or `approval_required`.
        reason: &'static str,
        /// Human-readable detail.
        message: String,
    },
    /// A meaningful fact changed since the plan was frozen (exit 8).
    #[error("{reason}")]
    Invalidated {
        /// What changed.
        reason: String,
    },
    /// The approval handle was rejected (exit 8).
    #[error("{0}")]
    Handle(HandleRefusal),
    /// Another submit holds the assignment (exit 8).
    #[error("in_progress")]
    InProgress {
        /// The journal that holds it, when one is known.
        journal_id: Option<String>,
    },
    /// Pre-flight failure at prepare or revalidation.
    #[error(transparent)]
    Preflight(#[from] PreflightError),
    /// Journal failure.
    #[error(transparent)]
    Journal(#[from] JournalError),
    /// Local persistence failure (exit 13).
    ///
    /// The conversion is hand-written below: a handle refusal travels through
    /// the store's error channel and must not be reported as persistence loss.
    #[error(transparent)]
    Store(DbError),
    /// JSON failure.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// I/O failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl PlanError {
    /// The `refused` reason this error carries, when it is a plan refusal.
    ///
    /// the design note maps every one of these to exit 8.
    #[must_use]
    pub fn refusal_reason(&self) -> Option<&str> {
        match self {
            Self::Refused { reason, .. } => Some(reason),
            // A plan that is gone can no longer be approved; it reads as
            // invalidated rather than as a missing local file.
            Self::Invalidated { .. } | Self::NotFound => Some("invalidated"),
            Self::Handle(_) => Some("approval_required"),
            _ => None,
        }
    }
}

impl From<DbError> for PlanError {
    fn from(value: DbError) -> Self {
        ops::handle_refusal_from(&value).map_or(Self::Store(value), Self::Handle)
    }
}
