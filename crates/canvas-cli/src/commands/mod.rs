//! Command modules.

pub mod alias;
pub mod announcement;
pub mod announcements;
pub mod assignment;
mod assignment_read;
pub mod assignments;
pub mod auth;
pub mod cache;
pub mod calendar;
pub mod completions;
pub mod config_cmd;
pub mod course;
pub mod course_load;
pub mod courses;
pub mod doctor;
pub mod download;
pub mod duration;
pub mod emit;
pub mod files;
pub mod identity;
pub mod modules;
pub mod open;
pub mod sync;
pub mod todo;
pub mod version;

use std::process::ExitCode;

use crate::output::ColorMode;
use crate::session::{Session, SessionError};

/// Stub answer for a command whose package has not landed yet: exit 1 with an
/// `error@1` envelope. Replaced arm by arm as each package lands.
pub fn not_implemented(json: bool) -> ExitCode {
    emit::emit_error(
        json,
        "not_implemented",
        "not implemented yet",
        1,
        None,
        None,
    )
}

/// Global CLI flags passed into command runners.
#[derive(Debug, Clone)]
#[allow(dead_code, clippy::struct_excessive_bools)]
pub struct Globals {
    pub json: bool,
    pub color: ColorMode,
    pub profile: Option<String>,
    pub fresh: bool,
    pub offline: bool,
    pub quiet: bool,
}

impl Globals {
    /// Open a local-only session (class A/B).
    pub fn open_local_session(&self) -> Result<Session, SessionError> {
        Session::open(self.profile.as_deref(), true)
    }

    /// Open a session with these globals.
    pub fn open_session(&self) -> Result<Session, SessionError> {
        Session::open(self.profile.as_deref(), self.offline)
    }
}
