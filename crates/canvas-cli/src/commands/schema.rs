//! `canvas schema` (§5, class A): the `--json` contract of a command.
//!
//! Raw output, like `completions`: one JSON Schema document, or the registry
//! listing, with no §7 envelope. `--json` is a usage error (exit 2), which
//! `Cli::validate` rejects before the command runs.

use std::io::{self, Write};
use std::process::ExitCode;

/// Print one command's schema document, or the whole registry.
pub fn run(command: Option<&str>, list: bool) -> ExitCode {
    let text = if list {
        crate::output::schema_list()
    } else {
        let name = command.unwrap_or_default();
        let Some(document) = crate::output::document_for_command(name) else {
            let _ = writeln!(
                io::stderr(),
                "no schema for {name:?}; run `canvas schema --list`"
            );
            return ExitCode::from(6);
        };
        match serde_json::to_string_pretty(&document) {
            Ok(mut text) => {
                text.push('\n');
                text
            }
            Err(e) => {
                let _ = writeln!(io::stderr(), "cannot encode the schema document: {e}");
                return ExitCode::from(1);
            }
        }
    };
    let mut stdout = io::stdout();
    if stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
        .is_err()
    {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
