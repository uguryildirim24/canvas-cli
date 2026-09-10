//! Workspace automation tasks.

mod bench;
mod bench_bridge;
mod bench_fixture;
mod bench_mcp;
mod dist_assets;
mod fixture;
mod record;
mod sanitize;

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
    /// Measure the SPEC section 13 targets against a fixture set.
    Bench {
        /// Fixture set under `crates/canvas-api/tests/fixtures/`.
        #[arg(long, value_name = "SET", default_value = bench::DEFAULT_SET)]
        fixture: String,
        /// Measured runs per metric.
        #[arg(long, default_value_t = 5)]
        runs: u32,
        /// Also measure the agent surface (`canvas mcp`).
        #[arg(long)]
        mcp: bool,
        /// Also measure the browser companion's broker (`canvas bridge host`).
        #[arg(long)]
        bridge: bool,
        /// Report a missed target without failing.
        #[arg(long)]
        no_fail: bool,
        /// Also measure one `watch` tick, and the targets with `watch` running.
        #[arg(long)]
        watch: bool,
        /// Write the report here instead of `docs/bench.md`.
        #[arg(long, value_name = "PATH")]
        doc: Option<PathBuf>,
    },
    /// Record live Canvas API fixtures. Not approved for a real account yet
    /// (SPEC section 19 item 5).
    Record {
        /// Canvas origin, for example `https://canvas.instructure.com`.
        #[arg(long, value_name = "ORIGIN")]
        host: String,
        /// Scratch directory to record into. The tracked fixture directory is
        /// refused; run `sanitize` to move a set there.
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
        /// Course to record the per-course endpoints for (repeatable).
        #[arg(long = "course", value_name = "ID")]
        courses: Vec<i64>,
    },
    /// Redact and pseudonymize a recorded set into the tracked fixtures.
    Sanitize {
        /// Recorded set to read.
        #[arg(long = "in", value_name = "DIR")]
        input: PathBuf,
        /// Fixture set to write.
        #[arg(long = "out", value_name = "DIR")]
        output: PathBuf,
    },
    /// Build distribution assets (man pages, completions).
    DistAssets {
        /// Directory to write `man/` and `completions/` into.
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
    },
    /// Add the man page and completion install lines to a `dist`-generated
    /// Homebrew formula, before it is committed to the tap.
    DistFormula {
        /// The `canvas-lms-cli.rb` `dist build --artifacts=global` produced.
        #[arg(long, value_name = "PATH")]
        formula: PathBuf,
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
        Command::DistFormula { formula } => match dist_assets::patch_formula(&formula) {
            Ok(true) => {
                println!("patched {}", formula.display());
                ExitCode::SUCCESS
            }
            Ok(false) => {
                println!(
                    "{} already installs man pages and completions",
                    formula.display()
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("dist-formula failed: {e:#}");
                ExitCode::from(1)
            }
        },
        Command::Record { host, out, courses } => {
            let task = record::Args { host, out, courses };
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("record failed: {e}");
                    return ExitCode::from(1);
                }
            };
            match runtime.block_on(record::run(&task)) {
                Ok(paths) => {
                    for path in paths {
                        println!("{}", path.display());
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("record failed: {e:#}");
                    ExitCode::from(1)
                }
            }
        }
        Command::Sanitize { input, output } => match sanitize::run(&input, &output) {
            Ok(names) => {
                for name in names {
                    println!("{}", output.join(name).display());
                }
                println!("{}", output.join(fixture::MANIFEST).display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("sanitize failed: {e:#}");
                ExitCode::from(1)
            }
        },
        Command::Bench {
            fixture,
            runs,
            mcp,
            bridge,
            no_fail,
            watch,
            doc,
        } => {
            let options = bench::Options {
                fixture,
                runs,
                no_fail,
                mcp,
                bridge,
                watch,
                doc,
            };
            match bench::run(&options) {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) => ExitCode::from(1),
                Err(e) => {
                    eprintln!("bench failed: {e:#}");
                    ExitCode::from(1)
                }
            }
        }
    }
}
