//! Submission journal state machine (§12.2).

mod locks;
mod ops;
mod record;
mod state;

pub use locks::{AdmissionLock, LockError, OwnerLock, probe_owner};
pub use ops::{
    CreateOpts, JournalError, JournalRow, PlanLink, TransitionPatch, acknowledge,
    append_uploaded_file_id, assume_not_submitted, commit_matched, commit_success, create,
    create_linked, enrich_readback, enrich_readback_full, get_journal, is_superseded, mark_posting,
    owner_status_for, recover_if_owner_absent, recover_owned, transition,
};
pub use record::{
    AttachmentRecord, CandidateRecord, Evidence, IntendedFile, IntendedPayload, IntendedText,
    PostedRecord, ReadbackRecord, ReceiptRecord, allowlist_from_json,
};
pub use state::{NotSubmittedEvidence, OwnerStatus, ResponseKind, State};

use crate::store::Store;

/// Doctor extension point from M0-c.
///
/// Full owner-absent recovery for a known journal id is [`recover_if_owner_absent`].
/// Doctor still calls this scan entry; returning an empty list keeps the M0-c
/// "skipped until wired" check until doctor passes an identity directory.
pub fn recover_owner_absent(_store: &Store) -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod crash_tests;
#[cfg(test)]
mod review_tests;
