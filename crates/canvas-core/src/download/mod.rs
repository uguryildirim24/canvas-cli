//! Module-aware file download planning and install.

pub mod contain;
pub mod install;
pub mod manifest;
pub mod plan;
pub mod sanitize;

pub use contain::{ContainError, ContainedPath, open_contained_file, walk_parent};
pub use install::{
    Action, ClassifyInput, ClobberDecision, FakeTransfer, InstallError, InstallOpts, MoveOutcome,
    RemoteMeta, Transfer, TransferError, classify_clobber, hash_path, install_part_file,
    makes_partial, outcome_is_partial, recover_pending_moves, resolve_move,
};
pub use manifest::{
    DestMeta, Destination, FORMAT_VERSION, InstallLock, LOCK_TIMEOUT, Manifest, ManifestError,
    ManifestRow, SCHEMA_USER_VERSION, open_destination,
};

pub use plan::{
    PlanCourse, PlanFile, PlanFolder, PlanInput, PlanModule, PlanModuleItem, PlannedFile,
    PlannedSource, plan_course,
};
pub use sanitize::{ComponentKind, sanitize_component, uniquify_paths};
