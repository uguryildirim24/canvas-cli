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

mod bridge;
mod commands;
mod config;
mod credentials;
mod exit;
mod mcp;
mod origin;
mod output;
mod paths;
mod selection;
mod session;
mod token;

use std::process::ExitCode;

use clap::Parser;

use cli::{AliasCommand, CacheCommand, Cli, ColorChoice, Commands, Globals, ReceiptsCommand};
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

/// Rewrite Chrome's native-messaging invocation into a command line.
///
/// The host manifest names the absolute path of the `canvas` binary (the design note
/// §3.4), and Chrome starts a native host as
/// `<path> chrome-extension://<id>/ [--parent-window=<handle>]`. Clap would
/// reject that first argument, so it is turned into the subcommand it means
/// before parsing. Nothing else is rewritten: only an argument that already
/// is a Chrome extension origin selects this path.
fn native_messaging_argv<I>(argv: I) -> Vec<std::ffi::OsString>
where
    I: IntoIterator<Item = std::ffi::OsString>,
{
    let mut argv: Vec<std::ffi::OsString> = argv.into_iter().collect();
    let is_caller = argv
        .get(1)
        .and_then(|arg| arg.to_str())
        .is_some_and(canvas_cli::cli::is_native_messaging_caller);
    if is_caller {
        argv.splice(1..1, ["bridge".into(), "host".into()]);
    }
    argv
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse_from(native_messaging_argv(std::env::args_os()));
    if let Err(error) = cli.validate() {
        error.exit();
    }
    let globals = Globals::from(&cli);
    let runtime = tokio::runtime::Handle::current();
    canvas_core::io::run_blocking(move || {
        runtime.block_on(async move {
            match cli.command {
                Commands::Version => commands::version::run(globals.json),
                Commands::Schema { command, list } => {
                    commands::schema::run(command.as_deref(), list)
                }
                Commands::Mcp => Box::pin(mcp::run(&m1b_globals(&globals))).await,
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
                Commands::Bridge { command } => match command {
                    cli::BridgeCommand::Host {
                        caller_origin,
                        parent_window: _,
                    } => Box::pin(bridge::host::run(&m1b_globals(&globals), caller_origin)).await,
                    cli::BridgeCommand::Install {
                        extension_id,
                        browser,
                    } => {
                        commands::bridge::run(
                            &m1b_globals(&globals),
                            commands::bridge::BridgeCmd::Install {
                                extension_id,
                                browser,
                            },
                        )
                        .await
                    }
                    cli::BridgeCommand::Status => {
                        commands::bridge::run(
                            &m1b_globals(&globals),
                            commands::bridge::BridgeCmd::Status,
                        )
                        .await
                    }
                    cli::BridgeCommand::Detach { attachment } => {
                        commands::bridge::run(
                            &m1b_globals(&globals),
                            commands::bridge::BridgeCmd::Detach {
                                attachment_id: attachment,
                            },
                        )
                        .await
                    }
                },
                Commands::Here { attachment, text } => {
                    commands::here::run(&m1b_globals(&globals), attachment, text).await
                }
                Commands::Sync { full } => commands::sync::run(&m1b_globals(&globals), full).await,
                Commands::Watch { jsonl, since, once } => {
                    commands::watch::run(&m1b_globals(&globals), jsonl, since, once).await
                }
                Commands::Notify { since, stdout } => {
                    commands::notify::run(&m1b_globals(&globals), since, stdout).await
                }
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
                Commands::Open {
                    command,
                    target,
                    follow,
                    attachment,
                } => {
                    let globals = m1b_globals(&globals);
                    if follow {
                        commands::open::follow(&globals, command, target, attachment, None, None)
                            .await
                            .emit(globals.json)
                    } else {
                        commands::open::run(&globals, command, target).await
                    }
                }
                Commands::Note {
                    attachment,
                    text,
                    source_refs,
                    generation,
                } => {
                    commands::note::run(
                        &m1b_globals(&globals),
                        attachment,
                        generation,
                        text,
                        source_refs,
                    )
                    .await
                }
                Commands::Grades { course, period } => {
                    commands::grades::run(&m1b_globals(&globals), course, period).await
                }
                Commands::Submit {
                    target,
                    assignment,
                    files,
                    text,
                    html,
                    url,
                    comment,
                    yes,
                } => {
                    commands::submit::run(
                        &m1b_globals(&globals),
                        commands::submit::SubmitArgs {
                            target,
                            assignment,
                            files,
                            text,
                            html,
                            url,
                            comment,
                            yes,
                        },
                    )
                    .await
                }
                Commands::Submission {
                    command,
                    target,
                    history,
                } => {
                    let cmd = match command {
                        Some(cli::SubmissionCommand::Verify { receipt_id }) => {
                            commands::submission::SubmissionCmd::Verify { receipt_id }
                        }
                        Some(cli::SubmissionCommand::Reconcile {
                            journal_id,
                            assume_not_submitted,
                        }) => commands::submission::SubmissionCmd::Reconcile {
                            journal_id,
                            assume_not_submitted,
                        },
                        None => {
                            let course = target.first().cloned().unwrap_or_default();
                            commands::submission::SubmissionCmd::Show {
                                course,
                                assignment: target.get(1).cloned(),
                                history,
                            }
                        }
                    };
                    commands::submission::run(&m1b_globals(&globals), cmd).await
                }
                Commands::Receipts { command } => {
                    let cmd = match command {
                        ReceiptsCommand::List { course, state } => {
                            commands::receipts::ReceiptsCmd::List { course, state }
                        }
                        ReceiptsCommand::Show { id } => {
                            commands::receipts::ReceiptsCmd::Show { id }
                        }
                        ReceiptsCommand::Export { receipt_id, out } => {
                            commands::receipts::ReceiptsCmd::Export { receipt_id, out }
                        }
                        ReceiptsCommand::Acknowledge { journal_id } => {
                            commands::receipts::ReceiptsCmd::Acknowledge { journal_id }
                        }
                    };
                    commands::receipts::run(&m1b_globals(&globals), cmd)
                }
                Commands::Announcements {
                    course,
                    since,
                    unread,
                } => {
                    commands::announcements::run(&m1b_globals(&globals), course, since, unread)
                        .await
                }
                Commands::Announcement { target, id } => {
                    commands::announcement::run(&m1b_globals(&globals), target, id).await
                }
                Commands::Pages {
                    course,
                    unpublished,
                } => commands::pages::run_list(&m1b_globals(&globals), course, unpublished).await,
                Commands::Page { course, page } => {
                    commands::pages::run_show(&m1b_globals(&globals), course, page).await
                }
                Commands::Syllabus { course } => {
                    commands::pages::run_syllabus(&m1b_globals(&globals), course).await
                }
                Commands::Discussions { course, unread } => {
                    commands::discussions::run_list(&m1b_globals(&globals), course, unread).await
                }
                Commands::Discussion {
                    command:
                        Some(cli::DiscussionCommand::Reply {
                            course,
                            discussion,
                            to,
                            text,
                            text_file,
                            attach,
                            yes,
                        }),
                    ..
                } => {
                    commands::operation::run_discussion_reply(
                        &m1b_globals(&globals),
                        commands::operation::DiscussionReplyArgs {
                            course,
                            discussion,
                            to,
                            text,
                            text_file,
                            attach,
                            yes,
                        },
                    )
                    .await
                }
                Commands::Discussion {
                    command: None,
                    course,
                    discussion,
                    replies,
                    page,
                } => {
                    commands::discussions::run_show(
                        &m1b_globals(&globals),
                        course.unwrap_or_default(),
                        discussion.unwrap_or_default(),
                        replies,
                        page,
                    )
                    .await
                }
                Commands::Inbox { command, scope } => match command {
                    Some(cli::InboxCommand::Show { id }) => {
                        commands::inbox::run_show(&m1b_globals(&globals), id).await
                    }
                    Some(cli::InboxCommand::UnreadCount) => {
                        commands::inbox::run_unread_count(&m1b_globals(&globals)).await
                    }
                    Some(cli::InboxCommand::Send {
                        to,
                        subject,
                        text,
                        text_file,
                        attach,
                        yes,
                    }) => {
                        commands::operation::run_inbox_send(
                            &m1b_globals(&globals),
                            commands::operation::InboxSendArgs {
                                to,
                                subject,
                                text,
                                text_file,
                                attach,
                                yes,
                            },
                        )
                        .await
                    }
                    Some(cli::InboxCommand::Reply {
                        conversation_id,
                        text,
                        text_file,
                        attach,
                        yes,
                    }) => {
                        commands::operation::run_inbox_reply(
                            &m1b_globals(&globals),
                            commands::operation::InboxReplyArgs {
                                conversation_id,
                                text,
                                text_file,
                                attach,
                                yes,
                            },
                        )
                        .await
                    }
                    None => commands::inbox::run_list(&m1b_globals(&globals), scope).await,
                },
                Commands::Quizzes { course } => {
                    commands::quizzes::run_list(&m1b_globals(&globals), course).await
                }
                Commands::NewQuizzes { course } => {
                    commands::new_quiz::run_list(&m1b_globals(&globals), course).await
                }
                Commands::NewQuiz { course, quiz } => {
                    commands::new_quiz::run_show(&m1b_globals(&globals), course, quiz).await
                }
                Commands::Quiz {
                    command: None,
                    course,
                    quiz,
                } => {
                    commands::quizzes::run_show(
                        &m1b_globals(&globals),
                        course.unwrap_or_default(),
                        quiz.unwrap_or_default(),
                    )
                    .await
                }
                Commands::Quiz {
                    command:
                        Some(cli::QuizCommand::Questions {
                            course,
                            quiz,
                            access_code,
                            yes,
                        }),
                    ..
                } => {
                    commands::quiz::run_questions(
                        &m1b_globals(&globals),
                        commands::quiz::QuizQuestionsArgs {
                            course,
                            quiz,
                            access_code,
                            yes,
                        },
                    )
                    .await
                }
                Commands::Quiz {
                    command:
                        Some(cli::QuizCommand::Submit {
                            course,
                            quiz,
                            answers,
                            access_code,
                            yes,
                        }),
                    ..
                } => {
                    commands::operation::run_quiz_submit(
                        &m1b_globals(&globals),
                        commands::operation::QuizSubmitArgs {
                            course,
                            quiz,
                            answers,
                            access_code,
                            yes,
                        },
                    )
                    .await
                }
                Commands::Operation { command } => match command {
                    cli::OperationCommand::Status { journal_id } => {
                        commands::operation::run_status(&m1b_globals(&globals), journal_id).await
                    }
                    cli::OperationCommand::Reconcile {
                        journal_id,
                        assume_not_posted,
                    } => {
                        commands::operation::run_reconcile(
                            &m1b_globals(&globals),
                            journal_id,
                            assume_not_posted,
                        )
                        .await
                    }
                },
                Commands::Calendar {
                    days,
                    course,
                    ics,
                    alarm,
                } => {
                    commands::calendar::run(
                        &m1b_globals(&globals),
                        commands::calendar::CalendarArgs {
                            days,
                            course,
                            ics,
                            alarm,
                        },
                    )
                    .await
                }
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

    use cli::{BridgeCommand, OpenCommand, SubmissionCommand};

    use super::*;

    /// M7-a: Chrome's own invocation reaches `bridge host`, and nothing else
    /// takes that path.
    #[test]
    fn chromes_native_messaging_invocation_becomes_bridge_host() {
        let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
        let argv = native_messaging_argv(
            ["canvas", origin, "--parent-window=0"]
                .into_iter()
                .map(Into::into),
        );
        let cli = Cli::try_parse_from(&argv).expect("chrome's command line parses");
        match cli.command {
            Commands::Bridge {
                command:
                    BridgeCommand::Host {
                        caller_origin,
                        parent_window,
                    },
            } => {
                assert_eq!(caller_origin.as_deref(), Some(origin));
                assert_eq!(parent_window.as_deref(), Some("0"));
            }
            other => panic!("unexpected {other:?}"),
        }
        // An ordinary command line is untouched.
        for plain in [
            vec!["canvas", "todo"],
            vec!["canvas", "bridge", "status"],
            vec!["canvas", "https://school.test/courses/1"],
        ] {
            let argv = native_messaging_argv(plain.iter().map(|a| (*a).into()));
            assert_eq!(argv.len(), plain.len(), "{plain:?} was rewritten");
        }
    }

    /// `bridge host` owns stdout for Chrome's framing, so `--json` on it is a
    /// usage error, not a second output contract.
    #[test]
    fn bridge_host_rejects_json() {
        let cli = Cli::try_parse_from(["canvas", "--json", "bridge", "host"]).expect("parses");
        assert!(cli.validate().is_err());
        let ok = Cli::try_parse_from(["canvas", "--json", "bridge", "status"]).expect("parses");
        ok.validate()
            .expect("status is an ordinary class-B command");
    }

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
                ..
            } => {
                assert_eq!(course, "chem");
                assert_eq!(assignment, "hw1");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
