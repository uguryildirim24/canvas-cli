//! `canvas` command-line entry point.

#![allow(
    clippy::too_many_lines,
    clippy::struct_excessive_bools,
    clippy::assigning_clones,
    clippy::struct_field_names,
    clippy::derivable_impls,
    clippy::needless_pass_by_value,
    clippy::format_collect,
    clippy::match_same_arms,
    clippy::collapsible_if,
    clippy::unused_async
)]

mod cli;
mod commands;
mod config;
mod credentials;
mod exit;
mod origin;
mod output;
mod paths;
mod selection;
mod token;

use std::io;
use std::process::ExitCode;

use clap::{CommandFactory, Parser};
use clap_complete::generate;

use cli::{Cli, Commands, Globals};

fn not_implemented() -> ExitCode {
    eprintln!("not implemented yet");
    ExitCode::from(1)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Err(error) = cli.validate() {
        error.exit();
    }
    let globals = Globals::from(&cli);
    let runtime = tokio::runtime::Handle::current();
    canvas_core::io::run_blocking(move || {
        runtime.block_on(async move {
            match cli.command {
                Commands::Version => {
                    println!("{}", env!("CARGO_PKG_VERSION"));
                    ExitCode::SUCCESS
                }
                Commands::Completions { shell } => {
                    let mut cmd = Cli::command();
                    generate(shell, &mut cmd, "canvas", &mut io::stdout());
                    ExitCode::SUCCESS
                }
                Commands::Auth { command } => match commands::auth::run(&globals, command).await {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => e.exit_with_json(globals.json),
                },
                Commands::Identity { command } => {
                    match commands::identity::run(&globals, command).await {
                        Ok(()) => ExitCode::SUCCESS,
                        Err(e) => e.exit_with_json(globals.json),
                    }
                }
                Commands::Doctor { network } => {
                    match commands::doctor::run(&globals, network).await {
                        Ok(()) => ExitCode::SUCCESS,
                        Err(e) => e.exit_with_json(globals.json),
                    }
                }
                Commands::Config { command } => {
                    match commands::config_cmd::run(&globals, command).await {
                        Ok(()) => ExitCode::SUCCESS,
                        Err(e) => e.exit_with_json(globals.json),
                    }
                }
                Commands::Courses { .. }
                | Commands::Course { .. }
                | Commands::Todo { .. }
                | Commands::Assignments { .. }
                | Commands::Assignment { .. }
                | Commands::Submit { .. }
                | Commands::Submission { .. }
                | Commands::Receipts { .. }
                | Commands::Grades { .. }
                | Commands::Files { .. }
                | Commands::Download { .. }
                | Commands::Modules { .. }
                | Commands::Announcements { .. }
                | Commands::Announcement { .. }
                | Commands::Calendar { .. }
                | Commands::Open { .. }
                | Commands::Sync { .. }
                | Commands::Cache { .. }
                | Commands::Alias { .. } => not_implemented(),
            }
        })
    })
    .await
    .unwrap_or_else(|_| {
        eprintln!("command worker failed");
        ExitCode::from(13)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cli::{OpenCommand, SubmissionCommand};

    #[test]
    fn global_flags_before_nested_commands_preserve_the_selected_command() {
        let cli = Cli::try_parse_from(["canvas", "submission", "--fresh", "verify", "receipt-1"])
            .unwrap();
        cli.validate().unwrap();
        assert!(matches!(
            cli.command,
            Commands::Submission {
                command: Some(SubmissionCommand::Verify { .. }),
                ..
            }
        ));

        let cli = Cli::try_parse_from(["canvas", "open", "--offline", "file", "123"]).unwrap();
        cli.validate().unwrap();
        assert!(matches!(
            cli.command,
            Commands::Open {
                command: Some(OpenCommand::File { .. }),
                ..
            }
        ));
    }
}
