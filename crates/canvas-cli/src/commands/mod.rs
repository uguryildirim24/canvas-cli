//! M1-b command implementations.

pub mod alias;
pub mod cache;
pub mod course;
pub mod course_load;
pub mod courses;
pub mod emit;
pub mod sync;

use crate::output::ColorMode;
use crate::session::{Session, SessionError};

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
    pub fn open_local_session(&self) -> Result<Session, SessionError> {
        Session::open(self.profile.as_deref(), true)
    }

    /// Open a session with these globals.
    pub fn open_session(&self) -> Result<Session, SessionError> {
        Session::open(self.profile.as_deref(), self.offline)
    }
}
