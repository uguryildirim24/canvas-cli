//! Submission journal state machine (§12.2).

mod locks;
mod ops;
mod record;
mod state;

pub use locks::{AdmissionLock, LockError, OwnerLock, probe_owner};
pub use ops::{
    CreateOpts, JournalError, JournalRow, TransitionPatch, acknowledge, append_uploaded_file_id,
    assume_not_submitted, commit_matched, commit_success, create, enrich_readback, get_journal,
    is_superseded, mark_posting, owner_status_for, recover_if_owner_absent, transition,
};
pub use record::{
    AttachmentRecord, CandidateRecord, Evidence, IntendedFile, IntendedPayload, IntendedText,
    PostedRecord, ReadbackRecord, ReceiptRecord, allowlist_from_json,
};
pub use state::{NotSubmittedEvidence, OwnerStatus, ResponseKind, State};

#[cfg(test)]
mod crash_tests;
#[cfg(test)]
mod review_tests;
