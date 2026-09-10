//! Distribution assets built from the clap command tree.
//!
//! `canvas completions <shell>` (§5) and `cargo xtask dist-assets` both go
//! through here, so the script a user sources is byte-identical to the script
//! the release archives carry.

use std::io::Write;

use clap::CommandFactory;
use clap_complete::{Generator, Shell};

use crate::cli::Cli;

/// Installed binary name (§4).
pub const BIN_NAME: &str = "canvas";

/// Version of the `canvas` binary these assets describe.
///
/// `xtask` builds the man pages and carries a version of its own, so it must
/// read this one rather than its own `CARGO_PKG_VERSION`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The shells §5 documents, in the order `dist-assets` writes them.
pub const SHELLS: &[Shell] = &[
    Shell::Bash,
    Shell::Zsh,
    Shell::Fish,
    Shell::PowerShell,
    Shell::Elvish,
];

/// The `canvas` command tree, with the binary name §4 fixes.
#[must_use]
pub fn command() -> clap::Command {
    Cli::command().name(BIN_NAME).bin_name(BIN_NAME)
}

/// Write the completion script for `shell`.
pub fn write_completions(shell: Shell, out: &mut impl Write) {
    clap_complete::generate(shell, &mut command(), BIN_NAME, out);
}

/// Conventional file name for a shell's completion script.
///
/// `canvas.bash`, `_canvas`, `canvas.fish`, `_canvas.ps1`, `canvas.elv`.
#[must_use]
pub fn completion_file_name(shell: Shell) -> String {
    shell.file_name(BIN_NAME)
}
