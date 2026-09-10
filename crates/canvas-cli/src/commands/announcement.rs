//! `canvas announcement` (§12.6, class C).

use std::process::ExitCode;

use super::Globals;

/// Run `canvas announcement <course> <id>` or `canvas announcement <url>`.
pub async fn run(globals: &Globals, _target: String, _id: Option<String>) -> ExitCode {
    super::not_implemented(globals.json)
}
