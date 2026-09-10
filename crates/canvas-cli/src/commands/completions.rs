//! `completions <shell>` (§5, class A): raw completion script on stdout.

use std::io::{self, Write};
use std::process::ExitCode;

use clap_complete::Shell;

/// Print the completion script for `shell`.
///
/// This is a raw-output command: `--json` is rejected by
/// [`crate::cli::Cli::validate`] with exit 2 before the command runs.
pub fn run(shell: Shell) -> ExitCode {
    let mut buffer = Vec::new();
    canvas_cli::dist::write_completions(shell, &mut buffer);
    let mut stdout = io::stdout().lock();
    match stdout.write_all(&buffer).and_then(|()| stdout.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        // A closed pipe (`canvas completions zsh | head`) is not an error.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            let _ = writeln!(io::stderr(), "failed to write completions: {e}");
            ExitCode::from(1)
        }
    }
}
