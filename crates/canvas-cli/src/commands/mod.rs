//! M2-b command implementations.

pub mod emit;
pub mod receipts;
pub mod submission;
pub mod submit;

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
    /// Open a session with these globals.
    pub fn open_session(&self) -> Result<Session, SessionError> {
        Session::open(self.profile.as_deref(), self.offline)
    }
}
