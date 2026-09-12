//! Discussion and inbox writes under a plan (M8-b).
//!
//! A discussion reply, a new conversation, and a reply to one are remote
//! writes, so each of them runs the plan layer of REPORT §3.5 —
//! `prepare → issue_handle → approve → execute` — and lands in a journal that
//! keeps SPEC §12.2's discipline. Nothing is sent without a recorded human
//! approval, one plan admits at most one journal, and an ambiguous outcome is
//! never resent.
//!
//! Three things differ from a submission, and each is deliberate.
//!
//! - **The target is a thread, not an assignment.** Admission is per topic,
//!   per conversation, or per new conversation, so a reply to one topic never
//!   blocks a reply to another.
//! - **Attribution is explicit and never inflated.** A 2xx that names an
//!   object is `accepted`; a readback that shows that object is `observed`; a
//!   readback that shows only a matching digest is `unproven`. Nothing else
//!   is claimed.
//! - **An acceptance is never delivery.** Canvas accepts a conversation. It
//!   does not report that a person received one, so the human line says
//!   "accepted by Canvas" and the receipt carries
//!   `delivery: "not_observable"`.

mod execute;
mod ops;
mod prepare;
mod reconcile;
mod record;

pub mod receipt;

#[cfg(test)]
mod crash_tests;
#[cfg(test)]
mod tests;

pub use execute::{Admitted, Posted, execute, post};
pub use ops::{
    ASSUME_AFTER, Patch, acknowledge, assume_not_posted, attach_identity, commit_matched,
    commit_observed, commit_posted, create_linked, enrich_readback, for_plan, get,
    identity_user_id, is_superseded, list, mark_posting, owner_status_for, pending,
    record_answers_response, record_attachment_id, recover_active, recover_if_owner_absent,
    recover_owned, require, transition,
};
pub use prepare::{
    DiscussionReplyRequest, InboxReplyRequest, InboxSendRequest, PreparedOperation,
    QuizSubmitRequest, Refusal, prepare_discussion_reply, prepare_inbox_reply, prepare_inbox_send,
    prepare_quiz_submit,
};
pub use reconcile::{Reconciled, Verdict, readback, reconcile, status};
pub use record::{
    Attribution, NotPostedEvidence, OpState, OperationAttachment, OperationKind, OperationLabels,
    OperationPlan, OperationReadback, OperationReceipt, OperationRow, OperationTarget,
    ResponseRecord, ServerMatch,
};

use thiserror::Error;

use crate::journal::LockError;
use crate::plan::PlanError;
use crate::store::DbError;

/// Operation-layer failures.
#[derive(Debug, Error)]
pub enum OperationError {
    /// No such operation journal.
    #[error("operation not found")]
    NotFound,
    /// Another process holds the target (exit 8).
    #[error("in_progress")]
    InProgress,
    /// An expected-state guard matched zero rows.
    #[error("state conflict")]
    StateConflict,
    /// The operation cannot be prepared as asked (exit 8, REPORT §3.2).
    #[error("{message}")]
    Refused {
        /// `group_write`, `locked`, `initial_post_required`, `unresolved`,
        /// `denied`, `empty_body`, or `unsupported`.
        reason: &'static str,
        /// Human-readable detail.
        message: String,
    },
    /// The target could not be resolved from what the caller named (exit 6).
    #[error("{0}")]
    Resolution(String),
    /// The plan layer refused (exit 8).
    #[error(transparent)]
    Plan(Box<PlanError>),
    /// A lock failure.
    #[error(transparent)]
    Lock(#[from] LockError),
    /// A network failure.
    #[error(transparent)]
    Network(#[from] canvas_api::Error),
    /// Local persistence failure (exit 13).
    #[error(transparent)]
    Store(DbError),
    /// JSON failure.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// I/O failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl OperationError {
    /// The `reason` this error carries when it is a refusal.
    ///
    /// A plan-layer refusal keeps its own reason (`expired`, `invalidated`,
    /// `approval_required`), because an operation plan is a plan: a caller
    /// that reads `reason` must get the same words either layer produced.
    #[must_use]
    pub fn refusal_reason(&self) -> Option<&str> {
        match self {
            Self::Refused { reason, .. } => Some(reason),
            Self::Plan(error) => error.refusal_reason(),
            _ => None,
        }
    }

    /// A refusal with a static reason and a message.
    #[must_use]
    pub fn refused(reason: &'static str, message: impl Into<String>) -> Self {
        Self::Refused {
            reason,
            message: message.into(),
        }
    }
}

impl From<PlanError> for OperationError {
    fn from(value: PlanError) -> Self {
        Self::Plan(Box::new(value))
    }
}

impl From<DbError> for OperationError {
    fn from(value: DbError) -> Self {
        if matches!(&value, DbError::Message(s) if s == "state conflict") {
            Self::StateConflict
        } else {
            Self::Store(value)
        }
    }
}

impl From<rusqlite::Error> for OperationError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Store(DbError::from(value))
    }
}
