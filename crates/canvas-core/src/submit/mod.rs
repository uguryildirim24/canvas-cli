//! Submit, reconcile, and verify orchestration (§12.2).

mod execute;
mod freeze;
mod preflight;
mod reconcile;
mod verify;

pub use execute::{ExecuteError, ExecuteOutcome, execute, post_and_finish};
pub use freeze::{
    FreezeError, FrozenInput, InputKind, TextSource, freeze_files, freeze_html, freeze_text,
    freeze_url, validate_comment,
};
pub use preflight::{
    AssignmentFacts, Plan, PreflightError, PreflightOutcome, admit, check_admissible,
    check_extensions, create_from_plan, enrich_payload, facts_of, fetch_assignment, preflight,
    preflight_with_input, recover_active,
};
pub(crate) use preflight::{freeze_on_worker, plan_of};
pub use reconcile::{
    ReconcileError, ReconcileOutcome, ReconcileResult, reconcile, reconcile_history,
};
pub use verify::{
    VerifyError, VerifyOutcome, VerifyResult, load_receipt_for_verify,
    validate_local as validate_receipt, verify,
};

use thiserror::Error;

use crate::journal::JournalError;

#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod tests;

/// Unified submit-domain error for callers that do not need finer mapping.
#[derive(Debug, Error)]
pub enum SubmitError {
    /// Validation failure (exit 2).
    #[error("{0}")]
    Validation(String),
    /// Network / API failure before a journal exists (exit 4).
    #[error(transparent)]
    Network(#[from] canvas_api::Error),
    /// In progress (exit 8).
    #[error("in_progress")]
    InProgress { journal_id: Option<String> },
    /// Refused (exit 8).
    #[error("{0}")]
    Refused(String),
    /// Recovery / unknown outcome (exit 9).
    #[error("{0}")]
    Recovery(String),
    /// Verify mismatch (exit 10).
    #[error("{0}")]
    Mismatch(String),
    /// Verify unavailable (exit 12).
    #[error("{0}")]
    Unavailable(String),
    /// State conflict (exit 13).
    #[error("state conflict")]
    StateConflict,
    /// Journal error.
    #[error(transparent)]
    Journal(#[from] JournalError),
    /// I/O.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl From<FreezeError> for SubmitError {
    fn from(value: FreezeError) -> Self {
        match value {
            FreezeError::Validation(s) => Self::Validation(s),
            FreezeError::Io(e) => Self::Io(e),
        }
    }
}

impl From<PreflightError> for SubmitError {
    fn from(value: PreflightError) -> Self {
        match value {
            PreflightError::Network(e) => Self::Network(e),
            PreflightError::InProgress { journal_id } => Self::InProgress { journal_id },
            PreflightError::Refused(s) => Self::Refused(s),
            PreflightError::Validation(s) => Self::Validation(s),
            PreflightError::Journal(e) => Self::Journal(e),
            PreflightError::Freeze(e) => e.into(),
            PreflightError::Io(e) | PreflightError::Lock(crate::journal::LockError::Io(e)) => {
                Self::Io(e)
            }
            PreflightError::Json(e) => Self::Json(e),
            PreflightError::Lock(crate::journal::LockError::InProgress) => {
                Self::InProgress { journal_id: None }
            }
        }
    }
}
