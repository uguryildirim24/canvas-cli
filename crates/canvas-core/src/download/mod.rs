//! Module-aware file download planning and install.

pub mod contain;
pub mod install;
pub mod manifest;
pub mod plan;
pub mod sanitize;
pub mod transport;

pub use contain::{ContainError, ContainedPath, open_contained_file, walk_parent};
pub use install::{
    Action, ClassifyInput, ClobberDecision, FakeTransfer, InstallError, InstallOpts, MoveOutcome,
    RemoteMeta, Transfer, TransferError, classify_clobber, hash_path, install_part_file,
    makes_partial, outcome_exit_code, outcome_is_partial, recover_pending_moves, resolve_move,
};
pub use manifest::{
    DestMeta, Destination, DestinationRecord, DestinationRegistry, FORMAT_VERSION, InstallLock,
    LOCK_TIMEOUT, Manifest, ManifestError, ManifestRow, SCHEMA_USER_VERSION,
    SqliteDestinationRegistry, open_destination,
};
pub use transport::{ApiTransfer, FileFetchMeta, ProgressFn, file_remote_meta};

pub use plan::{
    PlanCourse, PlanError, PlanFile, PlanFolder, PlanInput, PlanModule, PlanModuleItem,
    PlannedFile, PlannedSource, plan_course,
};
pub use sanitize::{ComponentKind, sanitize_component, uniquify_paths};
