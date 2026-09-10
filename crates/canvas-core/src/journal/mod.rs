//! Submission journal state machine (§12.2).

mod locks;
mod ops;
mod record;
mod state;

pub use locks::{AdmissionLock, LockError, OwnerLock, probe_owner};
pub use ops::{
    CreateOpts, JournalError, JournalRow, TransitionPatch, acknowledge, append_uploaded_file_id,
    commit_success, create, get_journal, mark_posting, owner_status_for, recover_if_owner_absent,
    transition,
};
pub use record::{
    AttachmentRecord, CandidateRecord, Evidence, PostedRecord, ReadbackRecord, ReceiptRecord,
    allowlist_from_json,
};
pub use state::{NotSubmittedEvidence, OwnerStatus, ResponseKind, State};
