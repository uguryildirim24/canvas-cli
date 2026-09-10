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

/// The clap definition lives in the library target so `xtask dist-assets`
/// builds man pages and completions from this exact command tree.
mod cli {
    pub use canvas_cli::cli::*;
}

mod commands;
mod config;
mod credentials;
mod exit;
mod origin;
mod output;
mod paths;
mod selection;
mod session;
mod token;

use std::process::ExitCode;

use clap::Parser;

use cli::{AliasCommand, CacheCommand, Cli, ColorChoice, Commands, Globals};
use output::ColorMode;

impl From<ColorChoice> for ColorMode {
    fn from(value: ColorChoice) -> Self {
        match value {
            ColorChoice::Auto => Self::Auto,
            ColorChoice::Always => Self::Always,
            ColorChoice::Never => Self::Never,
        }
    }
}

fn not_implemented(json: bool) -> ExitCode {
    commands::emit::emit_error(
        json,
        "not_implemented",
        "not implemented yet",
        1,
        None,
        None,
    )
}

/// Peer-lane Round-3 stub: exit 2 with `not implemented`.
fn not_implemented_r3(json: bool) -> ExitCode {
    commands::emit::emit_error(json, "not_implemented", "not implemented", 2, None, None)
}

fn m1b_globals(globals: &Globals) -> commands::Globals {
    commands::Globals {
        json: globals.json,
        color: globals.color.clone().unwrap_or(ColorChoice::Auto).into(),
        profile: globals.profile.clone(),
        fresh: globals.fresh,
        offline: globals.offline,
        quiet: globals.quiet,
    }
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
                Commands::Version => commands::version::run(globals.json),
                Commands::Completions { shell } => commands::completions::run(shell),
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
                Commands::Courses {
                    all,
                    term,
                    favorites,
                } => commands::courses::run(&m1b_globals(&globals), all, term, favorites).await,
                Commands::Course { course } => {
                    commands::course::run(&m1b_globals(&globals), course).await
                }
                Commands::Alias { command } => {
                    let cmd = match command {
                        AliasCommand::Set { name, course } => {
                            commands::alias::AliasCmd::Set { name, course }
                        }
                        AliasCommand::List => commands::alias::AliasCmd::List,
                        AliasCommand::Remove { name } => commands::alias::AliasCmd::Remove { name },
                    };
                    commands::alias::run(&m1b_globals(&globals), cmd).await
                }
                Commands::Sync { full } => commands::sync::run(&m1b_globals(&globals), full).await,
                Commands::Cache { command } => {
                    let cmd = match command {
                        CacheCommand::Stats => commands::cache::CacheCmd::Stats,
                        CacheCommand::Clear => commands::cache::CacheCmd::Clear,
                        CacheCommand::Path => commands::cache::CacheCmd::Path,
                    };
                    commands::cache::run(&m1b_globals(&globals), cmd).await
                }
                Commands::Files {
                    course,
                    tree,
                    search,
                } => commands::files::run(&m1b_globals(&globals), course, tree, search).await,
                Commands::Modules { course, items } => {
                    commands::modules::run(&m1b_globals(&globals), course, items).await
                }
                Commands::Download {
                    course,
                    all_courses,
                    dest,
                    module,
                    files,
                    jobs,
                    dry_run,
                    force,
                    verify,
                } => {
                    commands::download::run(
                        &m1b_globals(&globals),
                        commands::download::DownloadArgs {
                            course,
                            all_courses,
                            dest,
                            module,
                            files,
                            jobs,
                            dry_run,
                            force,
                            verify,
                        },
                    )
                    .await
                }
                Commands::Todo {
                    days,
                    all,
                    missing,
                    course,
                } => commands::todo::run(&m1b_globals(&globals), days, all, missing, course).await,
                Commands::Assignments {
                    course,
                    bucket,
                    search,
                } => {
                    commands::assignments::run(&m1b_globals(&globals), course, bucket, search).await
                }
                Commands::Assignment { target, assignment } => {
                    commands::assignment::run(&m1b_globals(&globals), target, assignment).await
                }
                Commands::Open { command, target } => {
                    commands::open::run(&m1b_globals(&globals), command, target).await
                }
                Commands::Grades { course, period } => {
                    commands::grades::run(&m1b_globals(&globals), course, period).await
                }
                Commands::Announcements { .. }
                | Commands::Announcement { .. }
                | Commands::Calendar { .. } => not_implemented(globals.json),
                // Round-3 peer-lane stubs (M2-b): exit 2.
                Commands::Submit { .. }
                | Commands::Submission { .. }
                | Commands::Receipts { .. } => not_implemented_r3(globals.json),
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
    use clap::Parser;

    use cli::{OpenCommand, SubmissionCommand};

    use super::*;

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

    #[test]
    fn submission_accepts_options_between_operands() {
        let cli = Cli::try_parse_from(["canvas", "submission", "chem", "--history", "hw1"])
            .expect("options between operands");
        match cli.command {
            Commands::Submission {
                command: None,
                target,
                history: true,
            } => assert_eq!(target, ["chem", "hw1"]),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn open_accepts_options_before_nested_operands() {
        let cli = Cli::try_parse_from(["canvas", "open", "--fresh", "assignment", "chem", "hw1"])
            .expect("global before nested");
        assert!(cli.fresh);
        match cli.command {
            Commands::Open {
                command: Some(OpenCommand::Assignment { course, assignment }),
                target: None,
            } => {
                assert_eq!(course, "chem");
                assert_eq!(assignment, "hw1");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
