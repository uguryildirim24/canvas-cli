//! Observations, baselines, and the `canvas-cli/event@1` log.
//!
//! A completed refresh is recorded as an observation, compared against the
//! baseline for its dataset scope, and turned into events. The first complete
//! observation of a scope only sets the baseline. Only a complete membership
//! of the same scope is compared, so a partial or failed page never implies a
//! removal. Event payloads carry allowlisted §12.2 fields, never a full
//! message, DOM text, a token, or a signed URL.
//!
//! Everything lives in `state.sqlite`, which `cache clear` does not touch.
//! Retention removes rows, never the file.

mod compare;
mod kind;
mod log;
mod observe;

#[cfg(test)]
mod tests;

pub use compare::{Member, Members, SHAPES, Shape, shape_for};
pub use kind::EventKind;
pub use log::{
    CursorCheck, EventIdentity, EventRecord, RETENTION_DAYS, check_cursor, consumer_cursor, expire,
    high_water, identity, insert_decision, low_water, read_after, record_operation_state,
    record_submission_state, reset_consumer_cursor, set_consumer_cursor,
};
pub use observe::{Observed, apply_pending, observe_refresh, record_observation};
