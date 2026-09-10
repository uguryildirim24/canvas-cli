//! Canvas CLI domain logic.

/// Local cache and state store.
pub mod store;

/// Identity and credential binding.
pub mod identity;

/// Dataset refresh and sync.
pub mod sync;

/// Course and target name resolution.
pub mod resolve;

/// Merged planner and missing-item todo view.
pub mod todo;

/// Submission journal state machine.
pub mod journal;

/// Local receipt export and verification helpers.
pub mod receipts;

/// Submit, reconcile, and verify orchestration.
pub mod submit;

/// Operation plans and the approval that admits them.
pub mod plan;

/// Module-aware file download planning and install.
pub mod download;

/// Calendar export helpers.
pub mod ics;

/// HTML and text transforms.
pub mod markdown;

/// Blocking I/O bridge onto the async runtime.
pub mod io;

/// Unique, self-cleaning temporary directories for tests.
///
/// Available to this crate's own tests and, through the `test-support`
/// feature, to the integration tests of crates that depend on it.
#[cfg(any(test, feature = "test-support"))]
pub mod test_scratch;
