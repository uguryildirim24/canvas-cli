//! `canvas announcements` (§12.6, class C).

use std::process::ExitCode;

use super::Globals;

/// Run `canvas announcements`.
pub async fn run(
    globals: &Globals,
    _course: Option<String>,
    _since: Option<String>,
    _unread: bool,
) -> ExitCode {
    super::not_implemented(globals.json)
}
