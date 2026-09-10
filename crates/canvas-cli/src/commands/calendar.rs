//! `canvas calendar` (§12.5, class C).

use std::process::ExitCode;

use super::Globals;

/// Operands and flags of `canvas calendar`.
#[derive(Debug, Clone, Default)]
// The stub reads none of these yet.
#[allow(dead_code)]
pub struct CalendarArgs {
    /// Window in days.
    pub days: Option<u32>,
    /// Course filter.
    pub course: Option<String>,
    /// iCalendar destination path, or `-` for stdout.
    pub ics: Option<String>,
    /// `VALARM` lead time.
    pub alarm: Option<String>,
}

/// Run `canvas calendar`.
pub async fn run(globals: &Globals, _args: CalendarArgs) -> ExitCode {
    super::not_implemented(globals.json)
}
