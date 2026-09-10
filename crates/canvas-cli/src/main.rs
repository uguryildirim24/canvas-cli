//! `canvas` command-line entry point.

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

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
  sync, cache stats|clear|path
  config path|edit|get|set
  alias set|list|remove
  doctor, completions, version
";

#[derive(Debug, Clone, ValueEnum)]
enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Parser)]
#[command(
    name = "canvas",
    version,
    about = "Command-line client for Canvas LMS",
    after_help = AFTER_HELP
)]
#[allow(clippy::struct_excessive_bools)]
struct Cli {
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    json: bool,

    /// Color output: auto, always, or never.
    #[arg(long, global = true, value_enum, default_value_t = ColorChoice::Auto)]
    color: ColorChoice,

    /// Named profile selection.
    #[arg(long, global = true)]
    profile: Option<String>,

    /// Ignore cache TTLs.
    #[arg(long, global = true, conflicts_with = "offline")]
    fresh: bool,

    /// Never touch the network.
    #[arg(long, global = true, conflicts_with = "fresh")]
    offline: bool,

    /// Quiet mode: no progress, no info logs.
    #[arg(short = 'q', long = "quiet", global = true)]
    quiet: bool,

    /// Verbose debug logs on stderr.
    #[arg(short = 'v', long = "verbose", global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
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
        #[arg(long)]
        bucket: Option<String>,
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
    Submission {
        #[command(subcommand)]
        command: SubmissionCommand,
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
    Download {
        /// Course id/code/alias, or unused when `--all-courses`.
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
        #[arg(long = "file")]
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
    Open {
        #[command(subcommand)]
        command: OpenCommand,
    },
    /// Refresh cached datasets.
    Sync {
        /// Also refresh files, modules, and calendar.
        #[arg(long)]
        full: bool,
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
    /// Generate shell completions.
    Completions {
        /// Shell to generate completions for.
        shell: Shell,
    },
    /// Print the `canvas` version.
    Version,
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
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
enum IdentityCommand {
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
enum SubmissionCommand {
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
    /// Show a submission: `submission <course> <assignment> [--history]`.
    #[command(external_subcommand)]
    #[allow(dead_code)]
    Show(Vec<String>),
}

#[derive(Debug, Subcommand)]
enum ReceiptsCommand {
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
enum OpenCommand {
    /// Open an assignment page.
    Assignment { course: String, assignment: String },
    /// Open a file by id.
    File { id: String },
    /// Open an announcement page.
    Announcement { course: String, id: String },
    /// Open a course or URL: `open <course|url>`.
    #[command(external_subcommand)]
    #[allow(dead_code)]
    Target(Vec<String>),
}

#[derive(Debug, Subcommand)]
enum CacheCommand {
    /// Show cache stats.
    Stats,
    /// Clear cache datasets.
    Clear,
    /// Print the cache path.
    Path,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
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
enum AliasCommand {
    /// Set a course alias.
    Set { name: String, course: String },
    /// List aliases.
    List,
    /// Remove an alias.
    Remove { name: String },
}

fn not_implemented() -> ExitCode {
    eprintln!("not implemented yet");
    ExitCode::from(1)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
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
        Commands::Auth { .. }
        | Commands::Identity { .. }
        | Commands::Courses { .. }
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
        | Commands::Config { .. }
        | Commands::Alias { .. }
        | Commands::Doctor { .. } => not_implemented(),
    }
}
