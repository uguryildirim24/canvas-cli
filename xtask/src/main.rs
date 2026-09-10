//! Workspace automation tasks.

mod dist_assets;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

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
    DistAssets {
        /// Directory to write `man/` and `completions/` into.
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
    },
}

fn main() -> ExitCode {
    let args = Args::parse();
    match args.command {
        Command::DistAssets { out } => match dist_assets::generate(&out) {
            Ok(assets) => {
                for path in assets.man_pages.iter().chain(&assets.completions) {
                    println!("{}", out.join(path).display());
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("dist-assets failed: {e:#}");
                ExitCode::from(1)
            }
        },
        Command::Bench | Command::Record | Command::Sanitize => {
            eprintln!("not implemented yet");
            ExitCode::from(1)
        }
    }
}
