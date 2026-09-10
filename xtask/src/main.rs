//! Workspace automation tasks.

use clap::{Parser, Subcommand};
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "canvas-cli workspace tasks")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run performance benchmarks.
    Bench,
    /// Record live Canvas API fixtures.
    Record,
    /// Sanitize recorded fixtures.
    Sanitize,
    /// Build distribution assets (man pages, completions).
    DistAssets,
}

fn main() -> ExitCode {
    let _args = Args::parse();
    eprintln!("not implemented yet");
    ExitCode::from(1)
}
