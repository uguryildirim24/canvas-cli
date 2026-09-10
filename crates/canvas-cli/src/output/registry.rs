//! Schema registry constants, M1-b result types, and fixtures.

use serde::{Deserialize, Serialize};

use crate::output::envelope::{Freshness, FreshnessSource, IdentityRef};

pub const SCHEMA_COURSES: &str = "canvas-cli/courses@1";
pub const SCHEMA_COURSE: &str = "canvas-cli/course@1";
pub const SCHEMA_TODO: &str = "canvas-cli/todo@1";
pub const SCHEMA_ASSIGNMENTS: &str = "canvas-cli/assignments@1";
pub const SCHEMA_ASSIGNMENT: &str = "canvas-cli/assignment@1";
pub const SCHEMA_SUBMIT: &str = "canvas-cli/submit@1";
pub const SCHEMA_SUBMISSION: &str = "canvas-cli/submission@1";
pub const SCHEMA_RECEIPT: &str = "canvas-cli/receipt@1";
pub const SCHEMA_RECEIPTS: &str = "canvas-cli/receipts@1";
pub const SCHEMA_VERIFY: &str = "canvas-cli/verify@1";
pub const SCHEMA_RECONCILE: &str = "canvas-cli/reconcile@1";
pub const SCHEMA_GRADES: &str = "canvas-cli/grades@1";
pub const SCHEMA_FILES: &str = "canvas-cli/files@1";
pub const SCHEMA_MODULES: &str = "canvas-cli/modules@1";
pub const SCHEMA_DOWNLOAD: &str = "canvas-cli/download@1";
pub const SCHEMA_ANNOUNCEMENTS: &str = "canvas-cli/announcements@1";
pub const SCHEMA_ANNOUNCEMENT: &str = "canvas-cli/announcement@1";
pub const SCHEMA_CALENDAR: &str = "canvas-cli/calendar@1";
pub const SCHEMA_OPEN: &str = "canvas-cli/open@1";
pub const SCHEMA_SYNC: &str = "canvas-cli/sync@1";
pub const SCHEMA_WATCH: &str = "canvas-cli/watch@1";
pub const SCHEMA_EVENT: &str = "canvas-cli/event@1";
pub const SCHEMA_CACHE: &str = "canvas-cli/cache@1";
pub const SCHEMA_ALIAS: &str = "canvas-cli/alias@1";
pub const SCHEMA_AUTH_STATUS: &str = "canvas-cli/auth_status@1";
pub const SCHEMA_AUTH_LOGIN: &str = "canvas-cli/auth_login@1";
pub const SCHEMA_AUTH_LOGOUT: &str = "canvas-cli/auth_logout@1";
pub const SCHEMA_IDENTITY: &str = "canvas-cli/identity@1";
pub const SCHEMA_CONFIG: &str = "canvas-cli/config@1";
pub const SCHEMA_DOCTOR: &str = "canvas-cli/doctor@1";
pub const SCHEMA_PLAN: &str = "canvas-cli/plan@1";
pub const SCHEMA_OPERATION: &str = "canvas-cli/operation@1";
pub const SCHEMA_OPERATION_RECONCILE: &str = "canvas-cli/operation_reconcile@1";
pub const SCHEMA_PAGES: &str = "canvas-cli/pages@1";
pub const SCHEMA_PAGE: &str = "canvas-cli/page@1";
pub const SCHEMA_SYLLABUS: &str = "canvas-cli/syllabus@1";
pub const SCHEMA_DISCUSSIONS: &str = "canvas-cli/discussions@1";
pub const SCHEMA_DISCUSSION: &str = "canvas-cli/discussion@1";
pub const SCHEMA_INBOX: &str = "canvas-cli/inbox@1";
pub const SCHEMA_CONVERSATION: &str = "canvas-cli/conversation@1";
pub const SCHEMA_INBOX_UNREAD: &str = "canvas-cli/inbox_unread@1";
pub const SCHEMA_HERE: &str = "canvas-cli/here@1";
pub const SCHEMA_NOTE: &str = "canvas-cli/note@1";
pub const SCHEMA_FOLLOW: &str = "canvas-cli/follow@1";
pub const SCHEMA_BRIDGE: &str = "canvas-cli/bridge@1";
pub const SCHEMA_VERSION: &str = "canvas-cli/version@1";
pub const SCHEMA_ERROR: &str = "canvas-cli/error@1";

/// One registered schema and its example `result` fixture JSON.
#[derive(Debug, Clone, Copy)]
pub struct SchemaEntry {
    pub id: &'static str,
    /// The command whose `--json` envelope this entry describes, as a person
    /// types it: `inbox show`, `submission reconcile`, `receipts export`.
    ///
    /// A command name cannot be derived from a schema id. `conversation@1` is
    /// printed by `inbox show` and `inbox_unread@1` by `inbox unread-count`,
    /// and deriving the name gave `conversation` and `inbox unread`, which are
    /// not commands anyone can run.
    ///
    /// `None` marks a document no command prints: `plan@1` is the agent
    /// surface's prepare result, `receipt@1` is the exported receipt file, and
    /// `error@1` is the branch any command takes when it aborts.
    pub command: Option<&'static str>,
    /// Which Appendix D `result` shape this fixture is.
    ///
    /// `None` for a schema with one shape. A schema whose Appendix D row lists
    /// several — `receipts@1`, `cache@1`, `config@1`, `identity@1` — has one
    /// entry per shape, named after the subcommand that emits it.
    pub variant: Option<&'static str>,
    pub fixture: &'static str,
}

/// Every Appendix D v1 schema with a fixture payload.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn all_schemas() -> &'static [SchemaEntry] {
    &[
        SchemaEntry {
            id: SCHEMA_COURSES,
            command: Some("courses"),
            variant: None,
            fixture: include_str!("schemas/courses.json"),
        },
        SchemaEntry {
            id: SCHEMA_COURSE,
            command: Some("course"),
            variant: None,
            fixture: include_str!("schemas/course.json"),
        },
        SchemaEntry {
            id: SCHEMA_TODO,
            command: Some("todo"),
            variant: None,
            fixture: include_str!("schemas/todo.json"),
        },
        SchemaEntry {
            id: SCHEMA_ASSIGNMENTS,
            command: Some("assignments"),
            variant: None,
            fixture: include_str!("schemas/assignments.json"),
        },
        SchemaEntry {
            id: SCHEMA_ASSIGNMENT,
            command: Some("assignment"),
            variant: None,
            fixture: include_str!("schemas/assignment.json"),
        },
        SchemaEntry {
            id: SCHEMA_SUBMIT,
            command: Some("submit"),
            variant: None,
            fixture: include_str!("schemas/submit.json"),
        },
        SchemaEntry {
            id: SCHEMA_PLAN,
            command: None,
            variant: None,
            fixture: include_str!("schemas/plan.json"),
        },
        SchemaEntry {
            id: SCHEMA_SUBMISSION,
            command: Some("submission"),
            variant: None,
            fixture: include_str!("schemas/submission.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECEIPT,
            command: None,
            variant: None,
            fixture: include_str!("schemas/receipt.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECEIPTS,
            command: Some("receipts list"),
            variant: Some("list"),
            fixture: include_str!("schemas/receipts.json"),
        },
        SchemaEntry {
            id: SCHEMA_VERIFY,
            command: Some("submission verify"),
            variant: None,
            fixture: include_str!("schemas/verify.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECONCILE,
            command: Some("submission reconcile"),
            variant: None,
            fixture: include_str!("schemas/reconcile.json"),
        },
        SchemaEntry {
            // One schema for four commands: the three writes and
            // `operation status` all print the operation journal as it
            // stands. `command` names the one a person is most likely to
            // look up, and `canvas schema` resolves the others through it.
            id: SCHEMA_OPERATION,
            command: Some("operation status"),
            variant: None,
            fixture: include_str!("schemas/operation.json"),
        },
        SchemaEntry {
            id: SCHEMA_OPERATION_RECONCILE,
            command: Some("operation reconcile"),
            variant: None,
            fixture: include_str!("schemas/operation_reconcile.json"),
        },
        SchemaEntry {
            id: SCHEMA_GRADES,
            command: Some("grades"),
            variant: None,
            fixture: include_str!("schemas/grades.json"),
        },
        SchemaEntry {
            id: SCHEMA_FILES,
            command: Some("files"),
            variant: None,
            fixture: include_str!("schemas/files.json"),
        },
        SchemaEntry {
            id: SCHEMA_MODULES,
            command: Some("modules"),
            variant: None,
            fixture: include_str!("schemas/modules.json"),
        },
        SchemaEntry {
            id: SCHEMA_DOWNLOAD,
            command: Some("download"),
            variant: None,
            fixture: include_str!("schemas/download.json"),
        },
        SchemaEntry {
            id: SCHEMA_ANNOUNCEMENTS,
            command: Some("announcements"),
            variant: None,
            fixture: include_str!("schemas/announcements.json"),
        },
        SchemaEntry {
            id: SCHEMA_ANNOUNCEMENT,
            command: Some("announcement"),
            variant: None,
            fixture: include_str!("schemas/announcement.json"),
        },
        SchemaEntry {
            id: SCHEMA_CALENDAR,
            command: Some("calendar"),
            variant: None,
            fixture: include_str!("schemas/calendar.json"),
        },
        SchemaEntry {
            id: SCHEMA_OPEN,
            command: Some("open"),
            variant: None,
            fixture: include_str!("schemas/open.json"),
        },
        SchemaEntry {
            id: SCHEMA_SYNC,
            command: Some("sync"),
            variant: None,
            fixture: include_str!("schemas/sync.json"),
        },
        SchemaEntry {
            id: SCHEMA_CACHE,
            command: Some("cache stats"),
            variant: Some("stats"),
            fixture: include_str!("schemas/cache_stats.json"),
        },
        SchemaEntry {
            id: SCHEMA_ALIAS,
            command: Some("alias list"),
            variant: None,
            fixture: include_str!("schemas/alias.json"),
        },
        SchemaEntry {
            id: SCHEMA_AUTH_STATUS,
            command: Some("auth status"),
            variant: None,
            fixture: include_str!("schemas/auth_status.json"),
        },
        SchemaEntry {
            id: SCHEMA_AUTH_LOGIN,
            command: Some("auth login"),
            variant: None,
            fixture: include_str!("schemas/auth_login.json"),
        },
        SchemaEntry {
            id: SCHEMA_AUTH_LOGOUT,
            command: Some("auth logout"),
            variant: None,
            fixture: include_str!("schemas/auth_logout.json"),
        },
        SchemaEntry {
            id: SCHEMA_IDENTITY,
            command: Some("identity list"),
            variant: Some("list"),
            fixture: include_str!("schemas/identity.json"),
        },
        SchemaEntry {
            id: SCHEMA_CONFIG,
            command: Some("config get"),
            variant: Some("get"),
            fixture: include_str!("schemas/config.json"),
        },
        SchemaEntry {
            id: SCHEMA_DOCTOR,
            command: Some("doctor"),
            variant: None,
            fixture: include_str!("schemas/doctor.json"),
        },
        SchemaEntry {
            id: SCHEMA_PAGES,
            command: Some("pages"),
            variant: None,
            fixture: include_str!("schemas/pages.json"),
        },
        SchemaEntry {
            id: SCHEMA_PAGE,
            command: Some("page"),
            variant: None,
            fixture: include_str!("schemas/page.json"),
        },
        SchemaEntry {
            id: SCHEMA_SYLLABUS,
            command: Some("syllabus"),
            variant: None,
            fixture: include_str!("schemas/syllabus.json"),
        },
        SchemaEntry {
            id: SCHEMA_DISCUSSIONS,
            command: Some("discussions"),
            variant: None,
            fixture: include_str!("schemas/discussions.json"),
        },
        SchemaEntry {
            id: SCHEMA_DISCUSSION,
            command: Some("discussion"),
            variant: None,
            fixture: include_str!("schemas/discussion.json"),
        },
        SchemaEntry {
            id: SCHEMA_INBOX,
            command: Some("inbox"),
            variant: None,
            fixture: include_str!("schemas/inbox.json"),
        },
        SchemaEntry {
            id: SCHEMA_CONVERSATION,
            command: Some("inbox show"),
            variant: None,
            fixture: include_str!("schemas/conversation.json"),
        },
        SchemaEntry {
            id: SCHEMA_INBOX_UNREAD,
            command: Some("inbox unread-count"),
            variant: None,
            fixture: include_str!("schemas/inbox_unread.json"),
        },
        SchemaEntry {
            id: SCHEMA_VERSION,
            command: Some("version"),
            variant: None,
            fixture: include_str!("schemas/version.json"),
        },
        SchemaEntry {
            id: SCHEMA_ERROR,
            command: None,
            variant: None,
            fixture: include_str!("schemas/error.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECEIPTS,
            command: Some("receipts show"),
            variant: Some("show"),
            fixture: include_str!("schemas/receipts_show.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECEIPTS,
            command: Some("receipts export"),
            variant: Some("export"),
            fixture: include_str!("schemas/receipts_export.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECEIPTS,
            command: Some("receipts acknowledge"),
            variant: Some("acknowledge"),
            fixture: include_str!("schemas/receipts_acknowledge.json"),
        },
        SchemaEntry {
            id: SCHEMA_CACHE,
            command: Some("cache clear"),
            variant: Some("clear"),
            fixture: include_str!("schemas/cache_clear.json"),
        },
        SchemaEntry {
            id: SCHEMA_CACHE,
            command: Some("cache path"),
            variant: Some("path"),
            fixture: include_str!("schemas/cache_path.json"),
        },
        SchemaEntry {
            id: SCHEMA_CONFIG,
            command: Some("config set"),
            variant: Some("set"),
            fixture: include_str!("schemas/config_set.json"),
        },
        SchemaEntry {
            id: SCHEMA_CONFIG,
            command: Some("config path"),
            variant: Some("path"),
            fixture: include_str!("schemas/config_path.json"),
        },
        SchemaEntry {
            id: SCHEMA_IDENTITY,
            command: Some("identity remove"),
            variant: Some("remove"),
            fixture: include_str!("schemas/identity_remove.json"),
        },
        SchemaEntry {
            id: SCHEMA_WATCH,
            command: Some("watch"),
            variant: None,
            fixture: include_str!("schemas/watch.json"),
        },
        SchemaEntry {
            id: SCHEMA_EVENT,
            // One line of the `watch --jsonl` stream, not a command envelope.
            command: None,
            variant: None,
            fixture: include_str!("schemas/event.json"),
        },
        SchemaEntry {
            id: SCHEMA_HERE,
            command: Some("here"),
            variant: None,
            fixture: include_str!("schemas/here.json"),
        },
        SchemaEntry {
            id: SCHEMA_NOTE,
            command: Some("note"),
            variant: None,
            fixture: include_str!("schemas/note.json"),
        },
        SchemaEntry {
            id: SCHEMA_FOLLOW,
            // Printed by `open --follow`, which is a flag on `open` and not a
            // command anyone types on its own.
            command: None,
            variant: None,
            fixture: include_str!("schemas/follow.json"),
        },
        SchemaEntry {
            id: SCHEMA_BRIDGE,
            command: Some("bridge status"),
            variant: Some("status"),
            fixture: include_str!("schemas/bridge_status.json"),
        },
        SchemaEntry {
            id: SCHEMA_BRIDGE,
            command: Some("bridge install"),
            variant: Some("install"),
            fixture: include_str!("schemas/bridge_install.json"),
        },
        SchemaEntry {
            id: SCHEMA_BRIDGE,
            command: Some("bridge detach"),
            variant: Some("detach"),
            fixture: include_str!("schemas/bridge_detach.json"),
        },
    ]
}

/// The registered schema a command name belongs to.
///
/// The name is the command path with any separator: `auth status`,
/// `auth_status`, and `auth-status` all name `canvas-cli/auth_status@1`.
///
/// A schema with several result shapes has one entry per shape, named after
/// the subcommand that emits it, so `receipts show` finds the `show` entry.
/// A bare `receipts` finds the first entry, which is what the bare command
/// prints.
#[must_use]
pub fn entry_for_command(name: &str) -> Option<&'static SchemaEntry> {
    let wanted = normalize_command(name);
    all_schemas()
        .iter()
        .find(|entry| normalize_command(&crate::output::entry_command(entry)) == wanted)
        .or_else(|| {
            all_schemas()
                .iter()
                .find(|entry| normalize_command(&crate::output::command_name(entry.id)) == wanted)
        })
}

/// The entry for one schema id and one of its result shapes.
///
/// `None` takes the first entry of that id, which is the shape the bare
/// command prints. A variant that is not registered resolves to nothing
/// rather than silently describing another shape.
#[must_use]
pub fn entry_for_schema(id: &str, variant: Option<&str>) -> Option<&'static SchemaEntry> {
    all_schemas()
        .iter()
        .find(|entry| entry.id == id && (variant.is_none() || entry.variant == variant))
}

fn normalize_command(name: &str) -> String {
    name.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Raw-output commands reject `--json` (SPEC §7):
/// `completions`, `notify`, `auth token --reveal`, `config edit`,
/// `calendar --ics -`, and `receipts export --out -`.
///
/// `watch` also rejects `--json`, for the other §7 reason: its stream is its
/// own contract, and `--jsonl` is the machine-readable form.
///
/// Pass `true` when the selected command is one of those raw-output forms.
#[must_use]
pub fn rejects_json(command_is_raw: bool) -> bool {
    command_is_raw
}

// --- M1-b typed result payloads (Appendix D) ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TermJson {
    pub id: Option<String>,
    pub name: Option<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PeriodJson {
    pub mode: String,
    pub id: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GradeJson {
    pub current_score: Option<f64>,
    pub current_grade: Option<String>,
    pub final_score: Option<f64>,
    pub final_grade: Option<String>,
    pub period: PeriodJson,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CourseJson {
    pub id: String,
    pub code: String,
    pub name: String,
    pub term: TermJson,
    pub enrollment_state: String,
    pub is_favorite: bool,
    pub restricted: bool,
    pub html_url: String,
    pub grades: GradeJson,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TeacherJson {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CourseDetailJson {
    pub id: String,
    pub code: String,
    pub name: String,
    pub term: TermJson,
    pub enrollment_state: String,
    pub is_favorite: bool,
    pub restricted: bool,
    pub html_url: String,
    pub grades: GradeJson,
    pub teachers: Vec<TeacherJson>,
    pub syllabus_markdown: Option<String>,
    pub time_zone: Option<String>,
    pub modules_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CoursesResult {
    pub courses: Vec<CourseJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CourseResult {
    pub course: CourseDetailJson,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AliasJson {
    pub name: String,
    pub course_id: String,
    pub course_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AliasResult {
    pub aliases: Vec<AliasJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SyncDatasetJson {
    pub dataset: String,
    pub scope: String,
    pub source: FreshnessSource,
    pub fetched_at: Option<String>,
    pub complete: bool,
    pub count: Option<u64>,
    pub stale: bool,
    pub requests: u64,
    pub error: Option<String>,
}

impl SyncDatasetJson {
    /// Convert to an envelope freshness row (drops sync-only fields).
    #[must_use]
    pub fn to_freshness(&self) -> Freshness {
        Freshness {
            dataset: self.dataset.clone(),
            scope: self.scope.clone(),
            source: self.source,
            fetched_at: self.fetched_at.clone(),
            complete: self.complete,
            count: self.count,
            stale: self.stale,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SyncResult {
    pub datasets: Vec<SyncDatasetJson>,
}

/// One `canvas-cli/event@1` line document (REPORT §3.6).
///
/// A stream line is self-describing: it names its own schema, so a consumer
/// that reads `canvas watch --jsonl` needs no envelope around it. §7 is
/// unchanged — this is the streaming contract, not a `--json` invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EventJson {
    pub schema: String,
    pub cursor: String,
    pub kind: String,
    pub observed_at: String,
    pub observed_at_local: Option<String>,
    pub identity: IdentityRef,
    pub generation: String,
    pub dataset: String,
    pub scope: String,
    pub entity_key: Option<String>,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
}

/// The `canvas watch --once` summary (`canvas-cli/watch@1`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WatchResult {
    /// The `--since` cursor the run replayed from, or null.
    pub since: Option<String>,
    /// The last cursor the run emitted, or null when it emitted nothing.
    pub cursor: Option<String>,
    /// Events written to the stream, replayed and new.
    pub events: u64,
    /// Ticks the run completed.
    pub ticks: u64,
    /// Whether the run emitted `resync_required` and closed.
    pub resync_required: bool,
    /// Why the last tick admitted no polling request, or null.
    pub skipped: Option<String>,
    /// Dataset coverage after the run, in `sync@1` rows.
    pub datasets: Vec<SyncDatasetJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CacheTableJson {
    pub name: String,
    pub rows: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CacheStatsResult {
    pub path: String,
    pub size_bytes: u64,
    pub tables: Vec<CacheTableJson>,
    pub datasets: Vec<Freshness>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CacheClearResult {
    pub cleared: bool,
    pub rows_deleted: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CachePathResult {
    pub path: String,
}

// --- M2-b typed result payloads (Appendix D) ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SubmitCandidateJson {
    pub attempt: i64,
    pub submitted_at: Option<String>,
    #[serde(default)]
    pub submitted_at_local: Option<String>,
    pub attachment_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SubmitFileJson {
    pub name: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub canvas_file_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SubmitTextJson {
    pub input_sha256: String,
    pub transform: String,
    pub sent_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SubmitResult {
    pub outcome: String,
    pub state: String,
    pub journal_id: String,
    /// True when this envelope reports a journal that already existed.
    ///
    /// `submission.execute` on a plan that is already executed returns the
    /// linked journal rather than a second one (SPEC §19 item 17). The human
    /// `submit` never reaches that path, so it always reports `false`.
    #[serde(default)]
    pub replayed: bool,
    #[serde(default)]
    pub receipt_id: Option<String>,
    #[serde(default)]
    pub attribution: Option<String>,
    #[serde(default)]
    pub post_status: Option<i64>,
    #[serde(default)]
    pub response_kind: Option<String>,
    #[serde(default)]
    pub posted: Option<serde_json::Value>,
    #[serde(default)]
    pub server_match: Option<SubmitCandidateJson>,
    pub candidates: Vec<SubmitCandidateJson>,
    pub files: Vec<SubmitFileJson>,
    #[serde(default)]
    pub text: Option<SubmitTextJson>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FilesListingJson {
    pub available: bool,
    pub http_status: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FileEntryJson {
    pub id: String,
    /// `"listing"` or `"module"`.
    pub source: String,
    pub folder_id: Option<String>,
    pub folder_path: Option<String>,
    pub module_id: Option<String>,
    pub module_position: Option<i64>,
    pub name: String,
    pub size: Option<u64>,
    pub updated_at: Option<String>,
    pub hidden: Option<bool>,
    pub locked: Option<bool>,
    pub lock_explanation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FilesResult {
    pub course_id: String,
    pub listing: FilesListingJson,
    pub files: Vec<FileEntryJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModuleItemJson {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub content_id: Option<String>,
    pub title: String,
    pub position: i64,
    pub locked: Option<bool>,
    pub lock_explanation: Option<String>,
    pub completed: Option<bool>,
    pub html_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModuleEntryJson {
    pub id: String,
    pub name: String,
    pub position: i64,
    pub state: Option<String>,
    pub items_count: Option<u64>,
    pub items_complete: bool,
    pub items: Vec<ModuleItemJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModulesResult {
    pub course_id: String,
    pub modules: Vec<ModuleEntryJson>,
}

// --- M4-a grades payloads (Appendix D `grades@1`) ---

/// Shared Appendix D `Course` object (grades are a sibling, not a member).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CourseBaseJson {
    pub id: String,
    pub code: String,
    pub name: String,
    pub term: TermJson,
    pub enrollment_state: String,
    pub is_favorite: bool,
    pub restricted: bool,
    pub html_url: String,
}

/// Appendix D `SubmissionStatus`; `missing` and `pending` are never null.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SubmissionStatusJson {
    pub submitted: Option<bool>,
    pub graded: Option<bool>,
    pub score: Option<f64>,
    pub grade: Option<String>,
    pub late: Option<bool>,
    pub missing: bool,
    pub excused: Option<bool>,
    pub workflow_state: Option<String>,
    pub submitted_at: Option<String>,
    pub submitted_at_local: Option<String>,
    pub attempt: Option<i64>,
    pub posted_at: Option<String>,
    pub pending: bool,
}

/// One course row of the grades overview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GradesCourseJson {
    pub course: CourseBaseJson,
    pub grades: GradeJson,
    /// Why this course has no total for the selected period, else `null`.
    pub unavailable_reason: Option<String>,
}

/// Canvas drop rules; `never_drop` is an array, never null.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GroupRulesJson {
    pub drop_lowest: Option<u32>,
    pub drop_highest: Option<u32>,
    pub never_drop: Vec<String>,
}

/// Group subtotal, present only when the API supplied one (SPEC §12.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GroupSubtotalJson {
    pub score: Option<f64>,
    pub possible: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GroupAssignmentJson {
    pub id: String,
    pub name: String,
    pub points_possible: Option<f64>,
    pub omit_from_final_grade: bool,
    pub status: SubmissionStatusJson,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AssignmentGroupJson {
    pub id: String,
    pub name: String,
    pub position: i64,
    pub weight: Option<f64>,
    pub rules: GroupRulesJson,
    pub subtotal: Option<GroupSubtotalJson>,
    pub assignments: Vec<GroupAssignmentJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GradingPeriodJson {
    pub id: String,
    pub title: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub is_current: bool,
}

/// The `grades <course>` course view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GradesCourseViewJson {
    pub groups: Vec<AssignmentGroupJson>,
    pub periods: Vec<GradingPeriodJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GradesResult {
    /// The selected period mode: `all`, `current`, or `id`.
    pub period_mode: String,
    pub courses: Vec<GradesCourseJson>,
    /// Present only for the course view; `null` for the overview.
    pub course: Option<GradesCourseViewJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DownloadFileJson {
    pub id: String,
    pub path: String,
    /// `null` unless `action` is `"moved"` (SPEC Appendix D: always present).
    pub previous_path: Option<String>,
    pub action: String,
    /// `null` when Canvas did not supply a size (SPEC Appendix D: always present).
    pub size: Option<u64>,
    /// `null` when the file needed no diagnostic (SPEC Appendix D: always present).
    pub error: Option<String>,
    /// `null` unless `--verify` checked this file (SPEC Appendix D: always present).
    pub verify: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DownloadCourseJson {
    pub course_id: String,
    pub course_code: String,
    pub files: Vec<DownloadFileJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DownloadTotalsJson {
    pub planned: u64,
    pub downloaded: u64,
    pub moved: u64,
    pub skipped: u64,
    pub unmanaged: u64,
    pub modified: u64,
    pub locked: u64,
    pub unavailable: u64,
    pub skipped_external: u64,
    pub unsafe_path: u64,
    pub unresolved_move: u64,
    pub failed: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DownloadResult {
    pub dest: String,
    pub dry_run: bool,
    pub courses: Vec<DownloadCourseJson>,
    pub totals: DownloadTotalsJson,
}

/// One frozen upload on a `plan@1` document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanFileJson {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

/// The text digests on a `plan@1` document.
///
/// The digests only. `plan@1` never carries the outbound bytes: `sent_sha256`
/// is what the approval binds, and §12.2 step 8 verifies it from the stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanTextJson {
    pub input_sha256: String,
    pub transform: String,
    pub sent_sha256: String,
}

/// The approval audit on a `plan@1` document; `null` before approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanApprovalJson {
    pub channel: String,
    pub at: String,
    pub consumer: Option<String>,
    pub plan_sha256: String,
}

/// A frozen plan (REPORT §3.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanJson {
    pub plan_id: String,
    pub state: String,
    pub consumer: Option<String>,
    /// Course id, or `null` for a plan that names no course.
    ///
    /// An inbox operation has no course at all, so both submission operands
    /// are nullable here rather than carrying a `0` that reads like an id.
    pub course_id: Option<String>,
    pub course_code: Option<String>,
    pub assignment_id: Option<String>,
    pub assignment_name: Option<String>,
    pub kind: String,
    pub baseline_attempt: i64,
    pub estimated_attempt: i64,
    pub files: Vec<PlanFileJson>,
    pub text: Option<PlanTextJson>,
    pub url: Option<String>,
    pub comment_chars: Option<u64>,
    pub due_at: Option<String>,
    pub plan_sha256: String,
    pub created_at: String,
    pub expires_at: String,
    pub approval: Option<PlanApprovalJson>,
    pub journal_id: Option<String>,
    pub invalidated_reason: Option<String>,
    /// The frozen write, for one of the three M8-b operation kinds.
    ///
    /// `null` for a submission plan; a submission's own fields are `null` for
    /// an operation plan. The kind says which half to read.
    pub operation: Option<PlanOperationJson>,
}

/// `plan@1` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanResult {
    pub plan: PlanJson,
}

impl PlanJson {
    /// Render a stored plan row.
    ///
    /// Digests and file hashes travel; the outbound bytes and the local file
    /// paths never do.
    #[must_use]
    pub fn of(row: &canvas_core::plan::PlanRow) -> Self {
        Self {
            plan_id: row.plan_id.clone(),
            state: row.state.as_str().to_owned(),
            consumer: row.consumer.clone(),
            course_id: operation_course(row).map(|id| id.to_string()),
            course_code: row
                .payload
                .course_code
                .clone()
                .or_else(|| operation_labels(row).and_then(|l| l.course_code.clone())),
            assignment_id: if row.kind.is_operation() {
                None
            } else {
                Some(row.assignment_id.to_string())
            },
            assignment_name: row.payload.assignment_name.clone(),
            kind: row.kind.as_str().to_owned(),
            baseline_attempt: row.baseline_attempt,
            estimated_attempt: row.baseline_attempt + 1,
            files: row
                .payload
                .files
                .iter()
                .map(|f| PlanFileJson {
                    name: f.name.clone(),
                    size: f.size,
                    sha256: f.sha256.clone(),
                })
                .collect(),
            text: row.payload.text.as_ref().map(|t| PlanTextJson {
                input_sha256: t.input_sha256.clone(),
                transform: t.transform.clone(),
                sent_sha256: t.sent_sha256.clone(),
            }),
            url: row.payload.url.clone(),
            comment_chars: row
                .payload
                .comment
                .as_ref()
                .map(|c| c.chars().count() as u64),
            due_at: row.payload.due_at.clone(),
            plan_sha256: row.plan_sha256.clone(),
            created_at: row.created_at.clone(),
            expires_at: row.expires_at.clone(),
            approval: row.approval.as_ref().map(|a| PlanApprovalJson {
                channel: a.channel.as_str().to_owned(),
                at: a.at.clone(),
                consumer: a.consumer.clone(),
                plan_sha256: a.plan_sha256.clone(),
            }),
            journal_id: row.journal_id.clone(),
            invalidated_reason: row.invalidated_reason.clone(),
            operation: row.operation.as_ref().map(PlanOperationJson::of),
        }
    }
}

/// The course a plan names, if it names one.
fn operation_course(row: &canvas_core::plan::PlanRow) -> Option<i64> {
    if row.kind.is_operation() {
        row.operation.as_ref().and_then(|op| op.target.course_id())
    } else {
        Some(row.course_id)
    }
}

fn operation_labels(
    row: &canvas_core::plan::PlanRow,
) -> Option<&canvas_core::operations::OperationLabels> {
    row.operation.as_ref().map(|op| &op.labels)
}

// --- M4-b typed result payloads (Appendix D) ---

/// Inclusive civil-day window shared by `announcements@1` and `calendar@1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WindowJson {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnnouncementJson {
    pub id: String,
    pub course_id: Option<String>,
    pub course_code: Option<String>,
    pub title: String,
    pub posted_at: Option<String>,
    pub posted_at_local: Option<String>,
    pub author: Option<String>,
    pub read: bool,
    pub html_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnnouncementsResult {
    pub window: WindowJson,
    pub announcements: Vec<AnnouncementJson>,
}

/// `announcement@1`: an `announcements@1` item plus the message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnnouncementDetailJson {
    #[serde(flatten)]
    pub item: AnnouncementJson,
    pub message_markdown: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnnouncementResult {
    pub announcement: AnnouncementDetailJson,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CalendarItemJson {
    /// `canvas-<kind>-<id>@<identity-key>`, the same UID the ICS carries.
    pub uid: String,
    pub kind: String,
    pub id: String,
    pub course_id: Option<String>,
    pub course_code: Option<String>,
    pub title: String,
    pub is_deadline: bool,
    pub due_at: Option<String>,
    pub due_at_local: Option<String>,
    pub start_at: Option<String>,
    pub start_at_local: Option<String>,
    pub end_at: Option<String>,
    pub end_at_local: Option<String>,
    pub all_day: bool,
    pub all_day_date: Option<String>,
    pub html_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CalendarResult {
    pub window: WindowJson,
    pub items: Vec<CalendarItemJson>,
}

// --- M8-b operation result payloads (Appendix D) ---

/// The thread or the recipients one operation writes to.
///
/// One shape covers all three kinds; the fields another kind does not have are
/// `null` or empty, as §7 requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OperationTargetJson {
    pub kind: String,
    pub course_id: Option<String>,
    pub course_code: Option<String>,
    pub topic_id: Option<String>,
    pub topic_title: Option<String>,
    pub parent_entry_id: Option<String>,
    pub conversation_id: Option<String>,
    pub conversation_subject: Option<String>,
    pub recipients: Vec<String>,
    pub recipient_names: Vec<String>,
}

/// One frozen attachment. The local path never travels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OperationAttachmentJson {
    pub name: String,
    pub size: u64,
    pub sha256: String,
    pub canvas_file_id: Option<String>,
}

/// The allowlisted record of what Canvas answered. Never the body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OperationResponseJson {
    pub id: Option<String>,
    pub conversation_id: Option<String>,
    pub created_at: Option<String>,
    pub created_at_local: Option<String>,
    pub user_id: Option<String>,
    pub body_sha256: Option<String>,
    pub attachment_ids: Vec<String>,
    pub response_sha256: Option<String>,
}

/// What a readback of the thread showed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OperationReadbackJson {
    pub read_at: String,
    pub id: Option<String>,
    pub created_at: Option<String>,
    pub created_at_local: Option<String>,
    pub user_id: Option<String>,
    pub body_sha256: Option<String>,
    pub attachment_ids: Vec<String>,
    pub scanned: u32,
    pub complete: bool,
}

/// A candidate a readback matched by digest, with no id link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OperationMatchJson {
    pub id: String,
    pub created_at: Option<String>,
    pub created_at_local: Option<String>,
    pub user_id: Option<String>,
    pub body_sha256: String,
}

/// The frozen operation inside a `plan@1` document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanOperationJson {
    pub kind: String,
    pub target: OperationTargetJson,
    pub subject: Option<String>,
    pub text: PlanTextJson,
    pub attachments: Vec<OperationAttachmentJson>,
}

impl OperationTargetJson {
    /// Render a frozen target with the labels preparing observed.
    #[must_use]
    pub fn of(
        target: &canvas_core::operations::OperationTarget,
        labels: &canvas_core::operations::OperationLabels,
        conversation_id: Option<&str>,
    ) -> Self {
        use canvas_core::operations::OperationTarget as T;
        let mut json = Self {
            kind: target.kind().as_str().to_owned(),
            course_id: target.course_id().map(|id| id.to_string()),
            course_code: labels.course_code.clone(),
            topic_id: None,
            topic_title: labels.topic_title.clone(),
            parent_entry_id: None,
            conversation_id: conversation_id.map(str::to_owned),
            conversation_subject: labels.conversation_subject.clone(),
            recipients: Vec::new(),
            recipient_names: labels.recipients.clone(),
        };
        match target {
            T::DiscussionReply {
                topic_id,
                parent_entry_id,
                ..
            } => {
                json.topic_id = Some(topic_id.to_string());
                json.parent_entry_id = parent_entry_id.map(|id| id.to_string());
            }
            T::InboxSend { recipients } => json.recipients.clone_from(recipients),
            T::InboxReply { conversation_id } => {
                json.conversation_id = Some(conversation_id.to_string());
            }
        }
        json
    }
}

impl OperationAttachmentJson {
    #[must_use]
    pub fn of(attachment: &canvas_core::operations::OperationAttachment) -> Self {
        Self {
            name: attachment.name.clone(),
            size: attachment.size,
            sha256: attachment.sha256.clone(),
            canvas_file_id: attachment.canvas_file_id.clone(),
        }
    }
}

impl OperationResponseJson {
    #[must_use]
    pub fn of(record: &canvas_core::operations::ResponseRecord) -> Self {
        Self {
            id: record.id.clone(),
            conversation_id: record.conversation_id.clone(),
            created_at: record.created_at.clone(),
            created_at_local: record.created_at_local.clone(),
            user_id: record.user_id.clone(),
            body_sha256: record.body_sha256.clone(),
            attachment_ids: record.attachment_ids.clone(),
            response_sha256: record.response_sha256.clone(),
        }
    }
}

impl OperationReadbackJson {
    #[must_use]
    pub fn of(readback: &canvas_core::operations::OperationReadback) -> Self {
        Self {
            read_at: readback.read_at.clone(),
            id: readback.id.clone(),
            created_at: readback.created_at.clone(),
            created_at_local: readback.created_at_local.clone(),
            user_id: readback.user_id.clone(),
            body_sha256: readback.body_sha256.clone(),
            attachment_ids: readback.attachment_ids.clone(),
            scanned: readback.scanned,
            complete: readback.complete,
        }
    }
}

impl OperationMatchJson {
    #[must_use]
    pub fn of(found: &canvas_core::operations::ServerMatch) -> Self {
        Self {
            id: found.id.clone(),
            created_at: found.created_at.clone(),
            created_at_local: found.created_at_local.clone(),
            user_id: found.user_id.clone(),
            body_sha256: found.body_sha256.clone(),
        }
    }
}

impl PlanOperationJson {
    #[must_use]
    pub fn of(plan: &canvas_core::operations::OperationPlan) -> Self {
        Self {
            kind: plan.kind().as_str().to_owned(),
            target: OperationTargetJson::of(&plan.target, &plan.labels, None),
            subject: plan.subject.clone(),
            text: PlanTextJson {
                input_sha256: plan.body.input_sha256.clone(),
                transform: plan.body.transform.clone(),
                sent_sha256: plan.body.sent_sha256.clone(),
            },
            attachments: plan
                .attachments
                .iter()
                .map(OperationAttachmentJson::of)
                .collect(),
        }
    }
}

/// `operation@1`: one operation journal as it stands.
///
/// The three write commands and `operation status` all print this document.
/// `delivery` is `not_observable` for both inbox kinds: Canvas accepts a
/// conversation, and no read of Canvas can say that a person received it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OperationResult {
    pub outcome: String,
    pub kind: String,
    pub state: String,
    pub journal_id: String,
    pub plan_id: String,
    pub replayed: bool,
    pub receipt_id: Option<String>,
    pub attribution: String,
    pub delivery: String,
    pub post_status: Option<i64>,
    pub response_kind: Option<String>,
    pub not_posted_evidence: Option<String>,
    pub target: OperationTargetJson,
    pub subject: Option<String>,
    pub text: PlanTextJson,
    pub attachments: Vec<OperationAttachmentJson>,
    pub response: Option<OperationResponseJson>,
    pub readback: Option<OperationReadbackJson>,
    pub server_match: Option<OperationMatchJson>,
    pub acknowledged_at: Option<String>,
    pub error: Option<String>,
}

impl OperationResult {
    /// Render one journal row. Nothing here carries the body.
    #[must_use]
    pub fn of(row: &canvas_core::operations::OperationRow) -> Self {
        let conversation_id = row
            .response
            .as_ref()
            .and_then(|r| r.conversation_id.clone());
        Self {
            outcome: String::new(),
            kind: row.kind.as_str().to_owned(),
            state: row.state.as_str().to_owned(),
            journal_id: row.journal_id.clone(),
            plan_id: row.plan_id.clone(),
            replayed: false,
            receipt_id: row.receipt_id(),
            attribution: row.attribution.as_str().to_owned(),
            delivery: row.delivery().to_owned(),
            post_status: row.post_status,
            response_kind: row.response_kind.clone(),
            not_posted_evidence: row.not_posted_evidence.map(|e| e.as_str().to_owned()),
            target: OperationTargetJson::of(
                &row.intended.target,
                &row.intended.labels,
                conversation_id.as_deref(),
            ),
            subject: row.intended.subject.clone(),
            text: PlanTextJson {
                input_sha256: row.intended.body.input_sha256.clone(),
                transform: row.intended.body.transform.clone(),
                sent_sha256: row.intended.body.sent_sha256.clone(),
            },
            attachments: row
                .intended
                .attachments
                .iter()
                .map(OperationAttachmentJson::of)
                .collect(),
            response: row.response.as_ref().map(OperationResponseJson::of),
            readback: row.readback.as_ref().map(OperationReadbackJson::of),
            server_match: row.server_match.as_ref().map(OperationMatchJson::of),
            acknowledged_at: row.acknowledged_at.clone(),
            error: row.error_text.clone(),
        }
    }
}

/// `operation_reconcile@1`: what a readback of one operation concluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OperationReconcileResult {
    pub outcome: String,
    pub journal_id: String,
    pub kind: String,
    pub state: String,
    pub verdict: String,
    pub owner: String,
    pub attribution: String,
    pub delivery: String,
    pub receipt_id: Option<String>,
    pub readback: Option<OperationReadbackJson>,
    pub server_match: Option<OperationMatchJson>,
    /// Whether `--assume-not-posted` is available now (§12.2's grace window).
    pub assume_not_posted_available: bool,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::output::envelope::{Envelope, IdentityRef};

    #[test]
    fn every_registered_schema_has_a_parseable_fixture() {
        use crate::output::now::with_canvas_now;
        with_canvas_now("2026-09-09T17:05:12Z", || {
            let mut rendered = Vec::new();
            let mut ids = std::collections::HashSet::new();
            for entry in all_schemas() {
                assert!(
                    ids.insert((entry.id, entry.variant)),
                    "duplicate schema {} variant {:?}",
                    entry.id,
                    entry.variant
                );
                let result: serde_json::Value =
                    serde_json::from_str(entry.fixture).unwrap_or_else(|e| {
                        panic!("fixture for {} is not JSON: {e}", entry.id);
                    });
                // A stream document is a whole line, not a `result` payload:
                // `canvas watch --jsonl` writes it as it stands (REPORT §3.6).
                if entry.id == SCHEMA_EVENT {
                    assert_eq!(result["schema"], entry.id);
                    let _: EventJson = serde_json::from_value(result.clone())
                        .expect("the event@1 fixture parses as its document type");
                    rendered.push(result);
                    continue;
                }
                let local = matches!(
                    entry.id,
                    SCHEMA_CONFIG | SCHEMA_IDENTITY | SCHEMA_VERSION | SCHEMA_DOCTOR | SCHEMA_ERROR
                );
                let mut env = Envelope::new(
                    entry.id,
                    if local { None } else { Some("default") },
                    if local {
                        None
                    } else {
                        Some(IdentityRef {
                            origin: "https://example.instructure.com".into(),
                            user_id: "1".into(),
                            key: "example.instructure.com-1-deadbeef".into(),
                        })
                    },
                )
                .with_result(result);
                if entry.id == SCHEMA_ERROR {
                    env.outcome = crate::output::Outcome::Error;
                    env.exit = 3;
                }
                let mut buf = Vec::new();
                env.write_json(&mut buf).unwrap();
                let parsed: serde_json::Value = serde_json::from_slice(&buf).unwrap();
                assert_eq!(parsed["schema"], entry.id);
                assert!(parsed.get("result").is_some(), "{}", entry.id);
                rendered.push(parsed);
            }
            insta::assert_json_snapshot!("registered_envelopes", rendered);
        });
    }

    #[test]
    fn plan_json_shows_the_digests_and_hashes_but_never_the_outbound_bytes() {
        use canvas_core::journal::{IntendedFile, IntendedPayload, IntendedText};
        use canvas_core::plan::{Approval, ApprovalChannel, Observations, PlanRow, PlanState};
        use canvas_core::submit::InputKind;

        let secret_body = "<p>the essay nobody else may read</p>";
        let row = PlanRow {
            plan_id: "plan-1".into(),
            identity_key: "example.instructure.com-1-deadbeef".into(),
            identity_generation: "generation-1".into(),
            consumer: Some("mcp".into()),
            course_id: 101,
            assignment_id: 202,
            kind: canvas_core::plan::PlanKind::Submission(InputKind::OnlineHtml),
            payload: IntendedPayload {
                files: vec![IntendedFile {
                    name: "essay.pdf".into(),
                    size: 24576,
                    sha256: "bb".repeat(32),
                    canvas_file_id: None,
                }],
                text: Some(IntendedText {
                    input_sha256: "aa".repeat(32),
                    transform: "html-verbatim".into(),
                    sent_sha256: "dd".repeat(32),
                    outbound_bytes: secret_body.into(),
                }),
                url: None,
                comment: Some("please regrade".into()),
                course_code: Some("CS-101".into()),
                assignment_name: Some("Essay 1".into()),
                due_at: Some("2026-09-15T23:59:59Z".into()),
                time_zone: Some("America/New_York".into()),
            },
            file_paths: vec!["/home/student/private/essay.pdf".into()],
            input_sha256: Some("aa".repeat(32)),
            sent_sha256: Some("dd".repeat(32)),
            baseline_attempt: 1,
            baseline_submission_id: Some(9001),
            observations: Observations::default(),
            plan_sha256: "cc".repeat(32),
            state: PlanState::Approved,
            created_at: "2026-09-09T16:04:40Z".into(),
            expires_at: "2026-09-09T16:19:40Z".into(),
            approval: Some(Approval {
                channel: ApprovalChannel::Elicitation,
                at: "2026-09-09T16:04:52Z".into(),
                consumer: Some("mcp".into()),
                plan_sha256: "cc".repeat(32),
            }),
            journal_id: None,
            invalidated_reason: None,
            operation: None,
        };

        let json = serde_json::to_string(&PlanResult {
            plan: PlanJson::of(&row),
        })
        .unwrap();

        // The exact body digest and the file hashes travel.
        assert!(json.contains(&"dd".repeat(32)), "sent_sha256 is missing");
        assert!(json.contains(&"aa".repeat(32)), "input_sha256 is missing");
        assert!(json.contains(&"bb".repeat(32)), "the file hash is missing");
        assert!(json.contains(&"cc".repeat(32)), "plan_sha256 is missing");
        assert!(json.contains("essay.pdf"));
        assert!(json.contains("elicitation"));
        assert!(json.contains("2026-09-09T16:19:40Z"), "expiry is missing");

        // The bytes themselves, and the local path they came from, do not.
        assert!(
            !json.contains(secret_body),
            "plan@1 leaked the outbound bytes"
        );
        assert!(!json.contains("nobody else may read"));
        assert!(
            !json.contains("/home/student"),
            "plan@1 leaked a local path"
        );
        assert!(!json.contains("outbound_bytes"));
        assert!(
            !json.contains("please regrade"),
            "plan@1 leaked the comment text"
        );
        assert!(json.contains(r#""comment_chars":14"#));
    }

    #[test]
    fn m1b_fixtures_deserialize_to_typed_results() {
        let courses: CoursesResult =
            serde_json::from_str(include_str!("schemas/courses.json")).unwrap();
        assert_eq!(courses.courses[0].id, "101");

        let course: CourseResult =
            serde_json::from_str(include_str!("schemas/course.json")).unwrap();
        assert_eq!(course.course.teachers[0].id, "55");

        let aliases: AliasResult =
            serde_json::from_str(include_str!("schemas/alias.json")).unwrap();
        assert_eq!(aliases.aliases[0].course_id, "101");

        let sync: SyncResult = serde_json::from_str(include_str!("schemas/sync.json")).unwrap();
        assert_eq!(sync.datasets[0].source, FreshnessSource::Network);

        let stats: CacheStatsResult =
            serde_json::from_str(include_str!("schemas/cache_stats.json")).unwrap();
        assert_eq!(stats.tables[0].rows, 10);

        let clear: CacheClearResult =
            serde_json::from_str(include_str!("schemas/cache_clear.json")).unwrap();
        assert!(clear.cleared);

        let path: CachePathResult =
            serde_json::from_str(include_str!("schemas/cache_path.json")).unwrap();
        assert!(!path.path.is_empty());

        let files: FilesResult = serde_json::from_str(include_str!("schemas/files.json")).unwrap();
        assert_eq!(files.course_id, "101");
        assert!(files.listing.available);

        let modules: ModulesResult =
            serde_json::from_str(include_str!("schemas/modules.json")).unwrap();
        assert_eq!(modules.course_id, "101");

        let download: DownloadResult =
            serde_json::from_str(include_str!("schemas/download.json")).unwrap();
        assert!(!download.dest.is_empty());
        assert_eq!(download.courses[0].files[0].action, "downloaded");

        let grades: GradesResult =
            serde_json::from_str(include_str!("schemas/grades.json")).unwrap();
        assert_eq!(grades.period_mode, "current");
        // Appendix D sorts courses by code, groups by position.
        assert_eq!(grades.courses[0].course.code, "CS-101");
        assert_eq!(
            grades.courses[1].unavailable_reason.as_deref(),
            Some("no current grading period total")
        );
        assert!(grades.courses[1].grades.current_score.is_none());
        let view = grades.course.as_ref().unwrap();
        assert_eq!(view.groups[0].position, 1);
        // A subtotal is present only when the API supplied one.
        assert!(view.groups[0].subtotal.is_some() && view.groups[1].subtotal.is_none());
        assert!(view.periods[0].is_current && !view.periods[1].is_current);
    }

    /// SPEC §7 and Appendix D: every field `grades@1` defines is present,
    /// with `null` for unknown. This lane owns the registry, so the schema it
    /// added is held to the same rule the `download@1` test states.
    #[test]
    fn grades_optional_fields_are_always_present_and_nullable() {
        const STATUS_KEYS: [&str; 13] = [
            "submitted",
            "graded",
            "score",
            "grade",
            "late",
            "missing",
            "excused",
            "workflow_state",
            "submitted_at",
            "submitted_at_local",
            "attempt",
            "posted_at",
            "pending",
        ];
        const GRADE_KEYS: [&str; 5] = [
            "current_score",
            "current_grade",
            "final_score",
            "final_grade",
            "period",
        ];

        // A row with nothing known still serializes every listed field.
        let unknown = SubmissionStatusJson {
            submitted: None,
            graded: None,
            score: None,
            grade: None,
            late: None,
            missing: false,
            excused: None,
            workflow_state: None,
            submitted_at: None,
            submitted_at_local: None,
            attempt: None,
            posted_at: None,
            pending: false,
        };
        let value = serde_json::to_value(&unknown).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(
            object.keys().map(String::as_str).collect::<HashSet<_>>(),
            STATUS_KEYS.into_iter().collect::<HashSet<_>>(),
        );
        for key in STATUS_KEYS {
            assert!(
                key == "missing" || key == "pending" || object[key].is_null(),
                "{key} must serialize as null",
            );
        }

        // The registered fixture carries them too, and round-trips unchanged.
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("schemas/grades.json")).unwrap();
        for course in fixture["courses"].as_array().unwrap() {
            assert!(course.get("unavailable_reason").is_some(), "{course}");
            let keys = course["grades"].as_object().unwrap().keys();
            assert_eq!(
                keys.map(String::as_str).collect::<HashSet<_>>(),
                GRADE_KEYS.into_iter().collect::<HashSet<_>>(),
            );
        }
        for group in fixture["course"]["groups"].as_array().unwrap() {
            assert!(group.get("subtotal").is_some(), "{group}");
            for assignment in group["assignments"].as_array().unwrap() {
                let keys = assignment["status"].as_object().unwrap().keys();
                assert_eq!(
                    keys.map(String::as_str).collect::<HashSet<_>>(),
                    STATUS_KEYS.into_iter().collect::<HashSet<_>>(),
                    "fixture status {assignment} is missing an Appendix D field",
                );
            }
        }
        let typed: GradesResult = serde_json::from_value(fixture.clone()).unwrap();
        assert_eq!(serde_json::to_value(&typed).unwrap(), fixture);
    }

    #[test]
    fn m4b_fixtures_deserialize_to_typed_results() {
        let announcements: AnnouncementsResult =
            serde_json::from_str(include_str!("schemas/announcements.json")).unwrap();
        assert_eq!(announcements.window.start, "2026-08-26");
        // Appendix D: `posted_at` desc, then id.
        assert_eq!(announcements.announcements[0].id, "9001");
        assert!(!announcements.announcements[0].read);

        let announcement: AnnouncementResult =
            serde_json::from_str(include_str!("schemas/announcement.json")).unwrap();
        assert_eq!(announcement.announcement.item.id, "1");
        assert!(announcement.announcement.message_markdown.is_none());

        let calendar: CalendarResult =
            serde_json::from_str(include_str!("schemas/calendar.json")).unwrap();
        assert_eq!(calendar.items.len(), 3);
        assert!(calendar.items[0].is_deadline);
        let all_day = calendar.items.last().unwrap();
        assert!(all_day.all_day && all_day.all_day_date.as_deref() == Some("2026-09-14"));
        assert!(
            all_day.uid.starts_with("canvas-event-701@"),
            "UID carries the identity key (§12.5)"
        );

        // Every Appendix D field stays present, and `null` means unknown.
        let value = serde_json::to_value(&calendar).unwrap();
        for key in ["due_at", "start_at", "end_at", "all_day_date", "html_url"] {
            assert!(
                value["items"][2].get(key).is_some(),
                "calendar item is missing {key}"
            );
        }
        assert_eq!(
            serde_json::to_value(&announcements).unwrap()["announcements"][1]["author"],
            serde_json::Value::Null
        );
    }

    /// SPEC §7 and Appendix D: a field defined in Appendix D is always
    /// present; `null` means unknown or not applicable. `download@1` carried
    /// `skip_serializing_if` on its four optional fields, which omitted them.
    #[test]
    fn download_optional_fields_are_always_present_and_nullable() {
        const FILE_KEYS: [&str; 7] = [
            "id",
            "path",
            "previous_path",
            "action",
            "size",
            "error",
            "verify",
        ];

        // A row with nothing supplied still serializes every listed field.
        let empty = DownloadFileJson {
            id: "50".into(),
            path: "CS-101-101/files/lec.pdf".into(),
            previous_path: None,
            action: "planned".into(),
            size: None,
            error: None,
            verify: None,
        };
        let value = serde_json::to_value(&empty).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(
            object.keys().map(String::as_str).collect::<HashSet<_>>(),
            FILE_KEYS.into_iter().collect::<HashSet<_>>(),
        );
        for key in ["previous_path", "size", "error", "verify"] {
            assert!(object[key].is_null(), "{key} must serialize as null");
        }

        // The registered fixture carries them too, and round-trips unchanged.
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("schemas/download.json")).unwrap();
        for course in fixture["courses"].as_array().unwrap() {
            for file in course["files"].as_array().unwrap() {
                let keys = file.as_object().unwrap().keys();
                assert_eq!(
                    keys.map(String::as_str).collect::<HashSet<_>>(),
                    FILE_KEYS.into_iter().collect::<HashSet<_>>(),
                    "fixture row {file} is missing an Appendix D field",
                );
            }
        }
        let typed: DownloadResult = serde_json::from_value(fixture.clone()).unwrap();
        assert_eq!(serde_json::to_value(&typed).unwrap(), fixture);

        // The fixture exercises both a populated and an absent optional set.
        let rows = &typed.courses[0].files;
        assert_eq!(
            rows[1].previous_path.as_deref(),
            Some("CS-101-101/files/notes.pdf")
        );
        assert!(rows[1].size.is_none() && rows[1].verify.is_none());
    }

    #[test]
    fn rejects_json_documents_raw_commands() {
        assert!(rejects_json(true));
        assert!(!rejects_json(false));
    }

    #[test]
    fn courses_envelope_snapshot() {
        use crate::output::now::with_canvas_now;
        with_canvas_now("2026-09-09T17:05:12Z", || {
            let result: CoursesResult =
                serde_json::from_str(include_str!("schemas/courses.json")).unwrap();
            let mut env = Envelope::new(
                SCHEMA_COURSES,
                Some("lasell"),
                Some(IdentityRef {
                    origin: "https://lasell.instructure.com".into(),
                    user_id: "12345".into(),
                    key: "lasell.instructure.com-12345-3f9a1c2e".into(),
                }),
            )
            .with_result(result);
            env.freshness.push(Freshness {
                dataset: "courses".into(),
                scope: "active".into(),
                source: FreshnessSource::Cache,
                fetched_at: Some("2026-09-09T16:00:00Z".into()),
                complete: true,
                count: Some(1),
                stale: false,
            });
            let json = serde_json::to_string_pretty(&env).unwrap();
            insta::assert_snapshot!(json);
        });
    }
}

// --- M8-a richer reads (Appendix D v2, `docs/reads-v2.md`) ---

/// One piece of content the Markdown cannot carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EmbeddedJson {
    /// `iframe`, `lti`, `video`, `audio`, or `unknown`.
    pub kind: String,
    pub src_origin: Option<String>,
    /// Always `unavailable`: this package never fetches embedded content.
    pub reported: String,
}

/// One same-origin Canvas file a body refers to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FileRefJson {
    pub file_id: String,
    pub name: Option<String>,
    pub url: String,
}

/// One reference that leaves the Canvas origin; never fetched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExternalLinkJson {
    pub url: String,
}

/// Listing row for `pages`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PageSummaryJson {
    pub id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub updated_at: Option<String>,
    pub published: Option<bool>,
    pub front_page: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PagesResult {
    pub course_id: String,
    pub listing: FilesListingJson,
    pub pages: Vec<PageSummaryJson>,
}

/// One page body with everything the Markdown could not show.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PageDetailJson {
    pub id: String,
    pub course_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub updated_at: Option<String>,
    pub published: Option<bool>,
    pub front_page: Option<bool>,
    pub locked_for_user: Option<bool>,
    pub html_url: Option<String>,
    pub body_markdown: Option<String>,
    /// True when the body was cut at 64 KiB.
    pub truncated: bool,
    pub embedded: Vec<EmbeddedJson>,
    pub files: Vec<FileRefJson>,
    pub external_links: Vec<ExternalLinkJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PageResult {
    pub page: PageDetailJson,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SyllabusResult {
    pub course_id: String,
    pub syllabus_markdown: Option<String>,
    pub truncated: bool,
    pub embedded: Vec<EmbeddedJson>,
    pub files: Vec<FileRefJson>,
    pub external_links: Vec<ExternalLinkJson>,
    pub updated_at: Option<String>,
}

/// One child topic of a group discussion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GroupTopicChildJson {
    pub id: Option<String>,
    pub group_id: Option<String>,
}

/// Listing row for `discussions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DiscussionSummaryJson {
    pub id: String,
    pub course_id: Option<String>,
    pub title: Option<String>,
    pub posted_at: Option<String>,
    pub last_reply_at: Option<String>,
    pub author: Option<String>,
    pub read_state: Option<String>,
    pub unread_count: Option<u64>,
    pub reply_count: Option<u64>,
    pub locked: Option<bool>,
    pub pinned: Option<bool>,
    pub is_announcement: Option<bool>,
    pub require_initial_post: Option<bool>,
    pub assignment_id: Option<String>,
    pub points_possible: Option<f64>,
    pub group_category_id: Option<String>,
    pub html_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DiscussionsResult {
    pub course_id: String,
    pub listing: FilesListingJson,
    pub discussions: Vec<DiscussionSummaryJson>,
}

/// One reply, top level or nested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DiscussionReplyJson {
    pub id: String,
    pub parent_id: Option<String>,
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    pub created_at: Option<String>,
    pub message_markdown: Option<String>,
    pub truncated: bool,
    pub read_state: Option<String>,
    pub replies_count: u64,
}

/// How much of a reply set this answer covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RepliesCoverageJson {
    pub pages_fetched: u32,
    pub complete: bool,
    /// `initial_post_required`, `page_failed`, `not_requested`, or `null`.
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DiscussionDetailJson {
    pub id: String,
    pub course_id: Option<String>,
    pub title: Option<String>,
    pub posted_at: Option<String>,
    pub last_reply_at: Option<String>,
    pub author: Option<String>,
    pub discussion_type: Option<String>,
    pub read_state: Option<String>,
    pub unread_count: Option<u64>,
    pub reply_count: Option<u64>,
    pub locked: Option<bool>,
    pub pinned: Option<bool>,
    pub is_announcement: Option<bool>,
    pub require_initial_post: Option<bool>,
    pub assignment_id: Option<String>,
    pub points_possible: Option<f64>,
    pub group_category_id: Option<String>,
    pub group_topic_children: Vec<GroupTopicChildJson>,
    pub html_url: Option<String>,
    pub message_markdown: Option<String>,
    pub truncated: bool,
    pub embedded: Vec<EmbeddedJson>,
    pub files: Vec<FileRefJson>,
    pub external_links: Vec<ExternalLinkJson>,
    pub replies: Vec<DiscussionReplyJson>,
    /// Which `--page` of replies `replies` holds. 1 when `--page` is absent.
    pub replies_page: u32,
    /// How many replies the covered set holds, across every page.
    ///
    /// `replies` shows at most one page of them, so a page past the end is an
    /// empty list beside a non-zero total, and a reader can tell that apart
    /// from a thread with no replies (SPEC §19 item 28).
    ///
    /// `null` when the thread was never read — `--replies` was not asked, so
    /// `replies_coverage.blocked` is `not_requested`. A count of 0 is an
    /// observation that the thread has no replies, and this field never
    /// reports one the command did not make.
    pub replies_total: Option<u32>,
    pub replies_coverage: RepliesCoverageJson,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DiscussionResult {
    pub discussion: DiscussionDetailJson,
    /// True while an operation journal for this target is unresolved (§10).
    ///
    /// The pending hook is the same one `submission@1` carries: while a write
    /// this CLI started is not resolved, a read of the same thread cannot say
    /// what is there, whatever the cache says.
    #[serde(default)]
    pub pending: bool,
    /// The unresolved operation journals, oldest first.
    #[serde(default)]
    pub pending_journals: Vec<String>,
}

/// One conversation participant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ParticipantJson {
    pub id: Option<String>,
    pub name: Option<String>,
}

/// One attachment on a conversation message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConversationAttachmentJson {
    pub file_id: Option<String>,
    pub name: Option<String>,
    pub size: Option<u64>,
}

/// Listing row for `inbox`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConversationSummaryJson {
    pub id: String,
    pub subject: Option<String>,
    pub workflow_state: Option<String>,
    pub last_message_at: Option<String>,
    pub message_count: Option<u64>,
    pub context_name: Option<String>,
    pub starred: Option<bool>,
    pub participants: Vec<ParticipantJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InboxResult {
    pub scope: String,
    pub listing: FilesListingJson,
    pub conversations: Vec<ConversationSummaryJson>,
    /// True while an operation journal for this target is unresolved (§10).
    ///
    /// The pending hook is the same one `submission@1` carries: while a write
    /// this CLI started is not resolved, a read of the same thread cannot say
    /// what is there, whatever the cache says.
    #[serde(default)]
    pub pending: bool,
    /// The unresolved operation journals, oldest first.
    #[serde(default)]
    pub pending_journals: Vec<String>,
}

/// One message inside a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConversationMessageJson {
    pub id: Option<String>,
    pub author_id: Option<String>,
    pub created_at: Option<String>,
    pub body: Option<String>,
    /// True when the body was cut at 64 KiB.
    pub truncated: bool,
    pub attachments: Vec<ConversationAttachmentJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConversationDetailJson {
    pub id: String,
    pub subject: Option<String>,
    pub workflow_state: Option<String>,
    pub last_message_at: Option<String>,
    pub context_name: Option<String>,
    pub participants: Vec<ParticipantJson>,
    pub messages: Vec<ConversationMessageJson>,
    /// False when the listing row was served and no message set was fetched.
    pub messages_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConversationResult {
    pub conversation: ConversationDetailJson,
    /// True while an operation journal for this target is unresolved (§10).
    ///
    /// The pending hook is the same one `submission@1` carries: while a write
    /// this CLI started is not resolved, a read of the same thread cannot say
    /// what is there, whatever the cache says.
    #[serde(default)]
    pub pending: bool,
    /// The unresolved operation journals, oldest first.
    #[serde(default)]
    pub pending_journals: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InboxUnreadResult {
    /// `null` when Canvas did not report a count.
    pub unread_count: Option<u64>,
    /// True while an operation journal for this target is unresolved (§10).
    ///
    /// The pending hook is the same one `submission@1` carries: while a write
    /// this CLI started is not resolved, a read of the same thread cannot say
    /// what is there, whatever the cache says.
    #[serde(default)]
    pub pending: bool,
    /// The unresolved operation journals, oldest first.
    #[serde(default)]
    pub pending_journals: Vec<String>,
}

// --- M7-a typed result payloads (`here@1`, `bridge@1`; REPORT §3.2, §3.3) ---

/// The identity a bundle belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HereIdentityJson {
    pub key: String,
    pub generation: String,
}

/// The API side of `ContextBundle@1`: whole §7 envelopes, each with its own
/// freshness. A browser extract never updates one of these (REPORT §3.1).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HereApiJson {
    /// A `canvas-cli/course@1` envelope, when the route named a course.
    pub course: Option<serde_json::Value>,
    /// A `canvas-cli/assignment@1` envelope, when the route named one.
    pub assignment: Option<serde_json::Value>,
    /// A `canvas-cli/announcement@1` envelope, when the route named a topic
    /// that Canvas serves as an announcement.
    pub announcement: Option<serde_json::Value>,
}

/// The browser side of `ContextBundle@1`.
///
/// Everything here is an observation of one document at one moment. `ttl_ms`
/// is zero: browser context is never cacheable (REPORT §3.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HereBrowserJson {
    pub origin: String,
    pub account: HereAccountJson,
    pub zone: String,
    pub page_kind: Option<String>,
    pub course_id: Option<String>,
    pub assignment_id: Option<String>,
    pub topic_id: Option<String>,
    pub quiz_id: Option<String>,
    pub page_url: Option<String>,
    pub url: Option<String>,
    pub title: Option<String>,
    pub document_id: String,
    pub frame_id: i64,
    pub navigation_generation: u64,
    pub observed_at: String,
    pub ttl_ms: u64,
    pub selection: Option<String>,
    pub text: Option<String>,
    pub selection_bytes: u64,
    pub text_bytes: u64,
    pub truncated: bool,
    /// Why `selection` and `text` are absent, when they are: `zone_opaque`,
    /// `validating`, or `account_mismatch`.
    pub content_reason: Option<String>,
    /// The last `context.follow` this consumer asked for, with the load
    /// outcome as it stands now. `null` when none was asked for.
    ///
    /// It lives in `browser` because it is an observation of the browser, and
    /// it is where the **load outcome** of a navigation is reported: the
    /// `follow@1` answer carries the dispatch acknowledgement alone
    /// (REPORT §3.2).
    pub follow: Option<FollowJson>,
    /// The notes the panel is holding for this attachment, oldest first.
    pub notes: Vec<NoteJson>,
}

/// One `context.follow`, from dispatch to load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FollowJson {
    pub request_id: String,
    pub url: String,
    /// The companion accepted the navigation. This is not a load.
    pub dispatched: bool,
    pub dispatched_at: String,
    /// How long the acknowledgement took, in milliseconds.
    pub dispatch_ms: u64,
    /// The navigation generation the request was bound to.
    pub generation: u64,
    /// `loaded`, `failed`, or `unknown`.
    pub load: String,
    /// When the load outcome was reported, when it was.
    pub load_at: Option<String>,
}

/// One inert note the panel is holding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NoteJson {
    pub note_id: String,
    /// Which consumer wrote it.
    pub consumer: String,
    /// Markdown source. The panel renders a sanitized subset of it; nothing
    /// downstream should treat it as HTML.
    pub text: String,
    pub source_refs: Vec<String>,
    pub at: String,
    /// The navigation generation the note was written against.
    pub generation: u64,
}

impl From<&canvas_core::bridge::ipc::FollowStatus> for FollowJson {
    fn from(follow: &canvas_core::bridge::ipc::FollowStatus) -> Self {
        Self {
            request_id: follow.request_id.clone(),
            url: follow.url.clone(),
            dispatched: follow.dispatched,
            dispatched_at: follow.dispatched_at.clone(),
            dispatch_ms: follow.dispatch_ms,
            generation: follow.generation,
            load: follow.load.as_str().to_owned(),
            load_at: follow.load_at.clone(),
        }
    }
}

impl From<&canvas_core::bridge::note::Note> for NoteJson {
    fn from(note: &canvas_core::bridge::note::Note) -> Self {
        Self {
            note_id: note.note_id.clone(),
            consumer: note.consumer.clone(),
            text: note.text.clone(),
            source_refs: note.source_refs.clone(),
            at: note.at.clone(),
            generation: note.generation,
        }
    }
}

/// The account the companion probed, verified against this identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HereAccountJson {
    pub user_id: String,
    pub observed_at: String,
}

/// `note@1` result: the note the panel is now holding.
///
/// The note is inert. It is displayed to the person and decides nothing: no
/// note can approve, decline, or cancel a plan (REPORT §3.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NoteResult {
    /// The attachment the note belongs to; `null` when none was held.
    pub attachment: Option<String>,
    /// The consumer that wrote it; `null` for `canvas note`.
    pub consumer: Option<String>,
    /// The note, when one was held.
    pub note: Option<NoteJson>,
    /// How many notes the panel now holds for this attachment.
    pub held: u64,
    /// Why no note was held, when none was.
    pub reason: Option<String>,
}

/// `follow@1` result: one navigation, dispatch and load kept apart.
///
/// `dispatch` is what this answer knows: the companion accepted the
/// navigation. `load` is `unknown` here by design, and the settled outcome
/// arrives later on the `here@1` bundle (REPORT §3.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FollowResult {
    /// The attachment whose tab was asked to move; `null` when none was.
    pub attachment: Option<String>,
    /// The consumer that asked; `null` for the CLI.
    pub consumer: Option<String>,
    /// What the target resolved to: `course`, `assignment`, `file`,
    /// `announcement`, or `url`.
    pub target_kind: String,
    /// The resolved id, when the target named one.
    pub id: String,
    /// The canonical Canvas URL the target resolved to.
    pub url: String,
    /// The dispatch acknowledgement, when the companion gave one.
    pub follow: Option<FollowJson>,
    /// Navigation is not a preview. Canvas' own page controllers run, and
    /// some of them write: opening a discussion marks it read (S13).
    pub side_effects: Vec<String>,
    /// Why the tab did not move, when it did not.
    pub reason: Option<String>,
}

/// `here@1` = `ContextBundle@1` (REPORT §3.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HereResult {
    /// The opaque attachment id, when one was resolved.
    pub attachment: Option<String>,
    /// `attached`, `validating`, `paused`, or `not_attached`.
    pub state: String,
    /// The consumer this bundle was resolved for; `null` for the CLI.
    pub consumer: Option<String>,
    pub identity: HereIdentityJson,
    pub api: HereApiJson,
    pub browser: Option<HereBrowserJson>,
    /// Why the bundle carries no browser context, when it does not.
    pub reason: Option<String>,
}

impl From<&canvas_core::bridge::ipc::Context> for HereBrowserJson {
    fn from(context: &canvas_core::bridge::ipc::Context) -> Self {
        fn name<T: Serialize>(value: &T) -> Option<String> {
            serde_json::to_value(value)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
        }
        Self {
            origin: context.origin.clone(),
            account: HereAccountJson {
                user_id: context.account.user_id.clone(),
                observed_at: context.account.observed_at.clone(),
            },
            zone: name(&context.zone).unwrap_or_else(|| "unknown".to_owned()),
            page_kind: context.page_kind.as_ref().and_then(name),
            course_id: context.course_id.clone(),
            assignment_id: context.assignment_id.clone(),
            topic_id: context.topic_id.clone(),
            quiz_id: context.quiz_id.clone(),
            page_url: context.page_url.clone(),
            url: context.url.clone(),
            title: context.title.clone(),
            document_id: context.document_id.clone(),
            frame_id: context.frame_id,
            navigation_generation: context.navigation_generation,
            observed_at: context.observed_at.clone(),
            ttl_ms: context.ttl_ms,
            selection: context.selection.clone(),
            text: context.text.clone(),
            selection_bytes: context.selection_bytes,
            text_bytes: context.text_bytes,
            truncated: context.truncated,
            content_reason: context.content_reason.map(|r| r.as_str().to_owned()),
            follow: context.follow.as_ref().map(FollowJson::from),
            notes: context.notes.iter().map(NoteJson::from).collect(),
        }
    }
}
