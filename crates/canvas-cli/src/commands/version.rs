//! `version` (§5, class A): `version@1` with the build's commit and target.

use std::io::{self, Write};
use std::process::ExitCode;

use crate::output::{Envelope, SCHEMA_VERSION};

/// Crate version of the running binary.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Git commit the binary was built from, `None` when the build had no git.
pub const COMMIT: Option<&str> = option_env!("CANVAS_COMMIT");

/// Target triple the binary was built for.
pub const TARGET: &str = env!("CANVAS_BUILD_TARGET");

/// Emit `version@1`: version, commit, target.
pub fn run(json: bool) -> ExitCode {
    let envelope = Envelope::new(SCHEMA_VERSION, None, None).with_result(serde_json::json!({
        "version": VERSION,
        "commit": COMMIT,
        "target": TARGET,
    }));
    super::emit::emit(json, &envelope, || {
        let mut stdout = io::stdout().lock();
        match COMMIT {
            Some(commit) => writeln!(stdout, "{VERSION} ({commit}, {TARGET})"),
            None => writeln!(stdout, "{VERSION} ({TARGET})"),
        }
    })
}
