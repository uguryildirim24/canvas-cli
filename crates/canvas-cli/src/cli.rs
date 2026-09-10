//! Clap CLI definition and shared globals.

use clap::{ArgGroup, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use std::path::PathBuf;

const AFTER_HELP: &str = "\
Commands (v1):
  auth login|status|logout|token
  identity list|remove
  courses, course, todo, assignments, assignment
  submit, submission, submission verify|reconcile
  receipts list|show|export|acknowledge
  grades, files, download, modules
  announcements, announcement, calendar
  open, open assignment|file|announcement
  bridge install|host|status|detach, here
  sync, watch, notify, cache stats|clear|path
  config path|edit|get|set
  alias set|list|remove
  doctor, completions, version
";

#[derive(Debug, Clone, ValueEnum)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

/// A Chromium-family browser `bridge install` can register the host with.
///
/// The per-user `NativeMessagingHosts` directory of each lives in
/// `crate::bridge::manifest`; the enum is here because it is part of the
/// command tree `xtask dist-assets` renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum Browser {
    Chrome,
    Chromium,
    Edge,
}

impl Browser {
    /// The name printed in messages and in `bridge@1`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Chromium => "chromium",
            Self::Edge => "edge",
        }
    }
}

#[derive(Debug, Clone, ValueEnum, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AssignmentBucket {
    Open,
    Upcoming,
    Overdue,
    Past,
    Undated,
    Unsubmitted,
    Ungraded,
    Future,
    All,
}

#[derive(Debug, Parser)]
#[command(
    name = "canvas",
    version,
    about = "Command-line client for Canvas LMS",
    after_help = AFTER_HELP
)]
#[allow(clippy::struct_excessive_bools)]
pub struct Cli {
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    pub json: bool,

    /// Color output: auto, always, or never.
    #[arg(long, global = true, value_enum)]
    pub color: Option<ColorChoice>,

    /// Named profile selection.
    #[arg(long, global = true)]
    pub profile: Option<String>,

    /// Ignore cache TTLs.
    #[arg(long, global = true, conflicts_with = "offline")]
    pub fresh: bool,

    /// Never touch the network.
    #[arg(long, global = true, conflicts_with = "fresh")]
    pub offline: bool,

    /// Quiet mode: no progress, no info logs.
    #[arg(short = 'q', long = "quiet", global = true)]
    pub quiet: bool,

    /// Verbose debug logs on stderr.
    #[arg(short = 'v', long = "verbose", global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Authenticate and manage tokens.
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// List or remove stored identities.
    Identity {
        #[command(subcommand)]
        command: IdentityCommand,
    },
    /// List courses.
    Courses {
        /// Include completed and invited courses.
        #[arg(long)]
        all: bool,
        /// Filter by term name substring.
        #[arg(long)]
        term: Option<String>,
        /// Favorites only.
        #[arg(long)]
        favorites: bool,
    },
    /// Show one course.
    Course {
        /// Course id, code, or alias.
        course: String,
    },
    /// Show what is due and what is missing.
    Todo {
        /// Planner window in days.
        #[arg(long)]
        days: Option<u32>,
        /// Show every item, including undated.
        #[arg(long)]
        all: bool,
        /// Show only missing items.
        #[arg(long)]
        missing: bool,
        /// Filter by course.
        #[arg(long)]
        course: Option<String>,
    },
    /// List assignments in a course.
    Assignments {
        /// Course id, code, or alias.
        course: String,
        /// Local bucket filter.
        #[arg(long, value_enum)]
        bucket: Option<AssignmentBucket>,
        /// Local name substring filter.
        #[arg(long)]
        search: Option<String>,
    },
    /// Show one assignment prompt.
    Assignment {
        /// Course id/code/alias, or a Canvas URL.
        target: String,
        /// Assignment id or name when `target` is a course.
        assignment: Option<String>,
    },
    /// Submit work to an assignment.
    #[command(group(ArgGroup::new("content").args(["files", "text", "html", "url"]).required(true)))]
    Submit {
        /// Course, assignment URL, or first target.
        target: String,
        /// Assignment when `target` is a course.
        assignment: Option<String>,
        /// File to upload (repeatable).
        #[arg(long = "file")]
        files: Vec<PathBuf>,
        /// Text body path or `-` for stdin.
        #[arg(long)]
        text: Option<String>,
        /// HTML body path.
        #[arg(long)]
        html: Option<PathBuf>,
        /// URL submission.
        #[arg(long)]
        url: Option<String>,
        /// Optional comment.
        #[arg(long)]
        comment: Option<String>,
        /// Skip confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Show or reconcile a submission.
    #[command(subcommand_negates_reqs = true)]
    Submission {
        #[command(subcommand)]
        command: Option<SubmissionCommand>,
        /// Course id/code/alias followed by assignment id/name (both required).
        #[arg(required = true, num_args = 1..=2, value_names = ["COURSE", "ASSIGNMENT"])]
        target: Vec<String>,
        /// Include submission history.
        #[arg(long)]
        history: bool,
    },
    /// Manage local submission receipts.
    Receipts {
        #[command(subcommand)]
        command: ReceiptsCommand,
    },
    /// Show grades as Canvas reports them.
    Grades {
        /// Optional course filter.
        course: Option<String>,
        /// Grading period: current, all, or an id.
        #[arg(long)]
        period: Option<String>,
    },
    /// List course files.
    Files {
        /// Course id, code, or alias.
        course: String,
        /// Tree view.
        #[arg(long)]
        tree: bool,
        /// Local name substring filter.
        #[arg(long)]
        search: Option<String>,
    },
    /// Download course files.
    #[command(group(ArgGroup::new("scope").args(["course", "all_courses"]).required(true)))]
    Download {
        /// Course id/code/alias (alternative to `--all-courses`).
        course: Option<String>,
        /// Download every active course.
        #[arg(long)]
        all_courses: bool,
        /// Destination directory.
        #[arg(long)]
        dest: Option<PathBuf>,
        /// Module name filter.
        #[arg(long)]
        module: Option<String>,
        /// Specific file ids.
        #[arg(long = "file", num_args = 1..)]
        files: Vec<i64>,
        /// Parallel job count.
        #[arg(long)]
        jobs: Option<u32>,
        /// Plan only.
        #[arg(long)]
        dry_run: bool,
        /// Replace owned files.
        #[arg(long)]
        force: bool,
        /// Verify after download.
        #[arg(long)]
        verify: bool,
    },
    /// List modules in a course.
    Modules {
        /// Course id, code, or alias.
        course: String,
        /// Include module items.
        #[arg(long)]
        items: bool,
    },
    /// List announcements.
    Announcements {
        /// Optional course filter.
        course: Option<String>,
        /// Only newer than this duration.
        #[arg(long)]
        since: Option<String>,
        /// Unread only.
        #[arg(long)]
        unread: bool,
    },
    /// Show one announcement.
    Announcement {
        /// Course id/code/alias, or a Canvas URL.
        target: String,
        /// Announcement id when `target` is a course.
        id: Option<String>,
    },
    /// List course pages.
    Pages {
        /// Course id, code, or alias.
        course: String,
        /// Also show unpublished pages.
        #[arg(long)]
        unpublished: bool,
    },
    /// Show one course page.
    Page {
        /// Course id, code, or alias.
        course: String,
        /// Page slug, page id, or a Canvas page URL.
        page: String,
    },
    /// Show the course syllabus.
    Syllabus {
        /// Course id, code, or alias.
        course: String,
    },
    /// List course discussions.
    Discussions {
        /// Course id, code, or alias.
        course: String,
        /// Unread only.
        #[arg(long)]
        unread: bool,
    },
    /// Show one discussion.
    Discussion {
        /// Course id, code, or alias.
        course: String,
        /// Discussion id, or a Canvas discussion URL.
        discussion: String,
        /// Also read the replies.
        #[arg(long)]
        replies: bool,
        /// Which page of replies to show (100 per page); needs --replies.
        #[arg(long)]
        page: Option<u32>,
    },
    /// Read the conversation inbox.
    Inbox {
        #[command(subcommand)]
        command: Option<InboxCommand>,
        /// Which list: inbox, unread, sent, or archived.
        #[arg(long)]
        scope: Option<String>,
    },
    /// Show calendar events.
    Calendar {
        /// Window in days.
        #[arg(long)]
        days: Option<u32>,
        /// Filter by course.
        #[arg(long)]
        course: Option<String>,
        /// Write iCalendar to a path or `-`.
        #[arg(long)]
        ics: Option<String>,
        /// VALARM duration.
        #[arg(long)]
        alarm: Option<String>,
    },
    /// Open a Canvas URL in the browser.
    #[command(subcommand_negates_reqs = true)]
    Open {
        #[command(subcommand)]
        command: Option<OpenCommand>,
        /// Course id, code, alias, or a Canvas URL.
        #[arg(required = true)]
        target: Option<String>,
    },
    /// Set up and inspect the browser companion broker.
    Bridge {
        #[command(subcommand)]
        command: BridgeCommand,
    },
    /// Show the attached Canvas page as a context bundle.
    Here {
        /// The attachment to read. The sole one is used when it is omitted.
        #[arg(long, value_name = "ID")]
        attachment: Option<String>,
        /// Also ask for the selected passage and the visible excerpt.
        #[arg(long)]
        text: bool,
    },
    /// Refresh cached datasets.
    Sync {
        /// Also refresh files, modules, and calendar.
        #[arg(long)]
        full: bool,
    },
    /// Stream local events as they are observed.
    Watch {
        /// One complete JSON document per line.
        #[arg(long)]
        jsonl: bool,
        /// Replay events after this cursor first.
        #[arg(long, value_name = "CURSOR")]
        since: Option<String>,
        /// Run one tick and exit.
        #[arg(long)]
        once: bool,
    },
    /// Post desktop notifications for observed events.
    Notify {
        /// Consume events after this cursor.
        #[arg(long, value_name = "CURSOR")]
        since: Option<String>,
        /// Write the notifications to stdout instead of the desktop.
        #[arg(long)]
        stdout: bool,
    },
    /// Inspect or clear the local cache.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
    /// Show or edit config.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Manage course aliases.
    Alias {
        #[command(subcommand)]
        command: AliasCommand,
    },
    /// Diagnose local setup.
    Doctor {
        /// Probe the network.
        #[arg(long)]
        network: bool,
    },
    /// Print the JSON Schema of a command's `--json` output.
    Schema {
        /// Command to describe, for example `todo` or `assignment`.
        #[arg(value_name = "COMMAND", required_unless_present = "list")]
        command: Option<String>,
        /// List every registered schema instead.
        #[arg(long, conflicts_with = "command")]
        list: bool,
    },
    /// Serve the Model Context Protocol over stdin and stdout.
    ///
    /// One instance serves one identity: bind it with `--profile`.
    Mcp,
    /// Generate shell completions.
    Completions {
        /// Shell to generate completions for.
        shell: Shell,
    },
    /// Print the `canvas` version.
    Version,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Store a personal access token.
    Login {
        /// Canvas host or origin.
        #[arg(long)]
        host: Option<String>,
        /// Read the token from stdin.
        #[arg(long)]
        token_stdin: bool,
        /// Rebind an existing profile.
        #[arg(long)]
        replace: bool,
    },
    /// Show auth status.
    Status,
    /// Remove the active token.
    Logout,
    /// Print token metadata (or the secret with `--reveal`).
    Token {
        /// Print the raw token.
        #[arg(long)]
        reveal: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum IdentityCommand {
    /// List identities.
    List,
    /// Remove an identity and its data.
    Remove {
        /// Identity key.
        identity_key: String,
        /// Skip confirmation.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum SubmissionCommand {
    /// Verify a receipt against Canvas.
    Verify {
        /// Receipt id.
        receipt_id: String,
    },
    /// Reconcile a journal after an interrupted submit.
    Reconcile {
        /// Journal id.
        journal_id: String,
        /// Assume not submitted after the grace window.
        #[arg(long)]
        assume_not_submitted: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ReceiptsCommand {
    /// List receipts.
    List {
        #[arg(long)]
        course: Option<String>,
        #[arg(long)]
        state: Option<String>,
    },
    /// Show one receipt or journal.
    Show {
        /// Receipt id or journal id.
        id: String,
    },
    /// Export a receipt JSON file.
    Export {
        receipt_id: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Acknowledge an unknown journal.
    Acknowledge { journal_id: String },
}

#[derive(Debug, Subcommand)]
pub enum OpenCommand {
    /// Open an assignment page.
    Assignment { course: String, assignment: String },
    /// Open a file by id.
    File { id: String },
    /// Open an announcement page.
    Announcement { course: String, id: String },
}

#[derive(Debug, Subcommand)]
pub enum BridgeCommand {
    /// Write the Chrome native-messaging host manifest for this user.
    Install {
        /// The extension id Chrome shows for the unpacked companion.
        #[arg(long, value_name = "ID")]
        extension_id: Option<String>,
        /// Which Chromium-family browser to install for.
        #[arg(long, value_enum)]
        browser: Option<Browser>,
    },
    /// Speak Chrome native messaging on stdin and stdout.
    ///
    /// Chrome starts this; it is not an interactive command.
    Host {
        /// The caller origin Chrome passes as the first argument.
        #[arg(value_name = "CALLER_ORIGIN")]
        caller_origin: Option<String>,
        /// Chrome passes this on Windows. It is accepted and ignored.
        #[arg(long, value_name = "HANDLE")]
        parent_window: Option<String>,
    },
    /// Report the manifest, the broker owner, and the attachment.
    Status,
    /// Ask the live owner to drop the attachment.
    Detach {
        /// The attachment to drop. The sole one is used when it is omitted.
        #[arg(long, value_name = "ID")]
        attachment: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum InboxCommand {
    /// Show one conversation and its messages.
    Show {
        /// Conversation id.
        id: String,
    },
    /// Show the unread conversation count.
    UnreadCount,
}

#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    /// Show cache stats.
    Stats,
    /// Clear cache datasets.
    Clear,
    /// Print the cache path.
    Path,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print the config file path.
    Path,
    /// Open the config file in an editor.
    Edit,
    /// Get a config value.
    Get { key: String },
    /// Set a config value.
    Set { key: String, value: String },
}

#[derive(Debug, Subcommand)]
pub enum AliasCommand {
    /// Set a course alias.
    Set { name: String, course: String },
    /// List aliases.
    List,
    /// Remove an alias.
    Remove { name: String },
}

/// Whether an argument is the caller origin Chrome passes a native host.
///
/// Chrome always spells it `chrome-extension://<id>/`; the id itself is
/// checked against the configured one before a message is read.
#[must_use]
pub fn is_native_messaging_caller(argument: &str) -> bool {
    argument.starts_with("chrome-extension://")
}

impl Cli {
    pub fn validate(&self) -> Result<(), clap::Error> {
        // Clap checks each command level before propagating global values.
        // Validate the final values too, so split-level flags still conflict.
        let conflict = if self.fresh && self.offline {
            Some("--fresh cannot be used with --offline")
        } else if self.json && self.command.has_raw_output() {
            Some("--json cannot be used with this raw-output command")
        } else if self.json && matches!(self.command, Commands::Watch { .. }) {
            // REPORT §3.6: the stream is its own contract, and §7's one
            // document per invocation rule is unchanged. `--jsonl` is the
            // machine-readable form of `watch`.
            Some("--json cannot be used with watch; use --jsonl")
        } else {
            match &self.command {
                Commands::Submission {
                    command: Some(_),
                    target,
                    history,
                } if !target.is_empty() || *history => Some(
                    "submission operands and --history cannot be used with verify or reconcile",
                ),
                Commands::Open {
                    command: Some(_),
                    target: Some(_),
                } => Some("an open target cannot be used with an open subcommand"),
                _ => None,
            }
        };
        if let Some(message) = conflict {
            return Err(Self::command().error(clap::error::ErrorKind::ArgumentConflict, message));
        }
        // A multi-value positional is checked per contiguous group by clap.
        // Allow options between groups, then require exactly two values overall.
        if matches!(&self.command, Commands::Submission { command: None, target, .. } if target.len() != 2)
        {
            return Err(Self::command().error(
                clap::error::ErrorKind::WrongNumberOfValues,
                "submission requires exactly two operands: COURSE and ASSIGNMENT",
            ));
        }
        Ok(())
    }
}

impl Commands {
    pub fn has_raw_output(&self) -> bool {
        match self {
            // `bridge host` speaks Chrome's native-messaging framing on
            // stdout, not the §7 output contract (REPORT §3.2).
            Self::Bridge {
                command: BridgeCommand::Host { .. },
            }
            | Self::Completions { .. }
            // Notify posts derived alerts, not a query result, so it has no
            // §7 payload and no Appendix D row (REPORT §3.6).
            | Self::Notify { .. }
            | Self::Schema { .. }
            | Self::Auth {
                command: AuthCommand::Token { reveal: true },
            }
            | Self::Config {
                command: ConfigCommand::Edit,
            } => true,
            Self::Calendar {
                ics: Some(path), ..
            } => path == "-",
            Self::Receipts {
                command:
                    ReceiptsCommand::Export {
                        out: Some(path), ..
                    },
            } => path.as_os_str() == "-",
            _ => false,
        }
    }
}

/// Shared global flags passed to command handlers.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Globals {
    pub json: bool,
    pub color: Option<ColorChoice>,
    pub profile: Option<String>,
    pub fresh: bool,
    pub offline: bool,
    pub quiet: bool,
    pub verbose: bool,
}

impl From<&Cli> for Globals {
    fn from(cli: &Cli) -> Self {
        Self {
            json: cli.json,
            color: cli.color.clone(),
            profile: cli.profile.clone(),
            fresh: cli.fresh,
            offline: cli.offline,
            quiet: cli.quiet,
            verbose: cli.verbose,
        }
    }
}
