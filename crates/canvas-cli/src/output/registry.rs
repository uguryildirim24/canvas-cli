//! Schema registry constants, M1-b result types, and fixtures.

use serde::{Deserialize, Serialize};

use crate::output::envelope::{Freshness, FreshnessSource};

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
pub const SCHEMA_CACHE: &str = "canvas-cli/cache@1";
pub const SCHEMA_ALIAS: &str = "canvas-cli/alias@1";
pub const SCHEMA_AUTH_STATUS: &str = "canvas-cli/auth_status@1";
pub const SCHEMA_AUTH_LOGIN: &str = "canvas-cli/auth_login@1";
pub const SCHEMA_AUTH_LOGOUT: &str = "canvas-cli/auth_logout@1";
pub const SCHEMA_IDENTITY: &str = "canvas-cli/identity@1";
pub const SCHEMA_CONFIG: &str = "canvas-cli/config@1";
pub const SCHEMA_DOCTOR: &str = "canvas-cli/doctor@1";
pub const SCHEMA_PLAN: &str = "canvas-cli/plan@1";
pub const SCHEMA_VERSION: &str = "canvas-cli/version@1";
pub const SCHEMA_ERROR: &str = "canvas-cli/error@1";

/// One registered schema and its example `result` fixture JSON.
#[derive(Debug, Clone, Copy)]
pub struct SchemaEntry {
    pub id: &'static str,
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
            variant: None,
            fixture: include_str!("schemas/courses.json"),
        },
        SchemaEntry {
            id: SCHEMA_COURSE,
            variant: None,
            fixture: include_str!("schemas/course.json"),
        },
        SchemaEntry {
            id: SCHEMA_TODO,
            variant: None,
            fixture: include_str!("schemas/todo.json"),
        },
        SchemaEntry {
            id: SCHEMA_ASSIGNMENTS,
            variant: None,
            fixture: include_str!("schemas/assignments.json"),
        },
        SchemaEntry {
            id: SCHEMA_ASSIGNMENT,
            variant: None,
            fixture: include_str!("schemas/assignment.json"),
        },
        SchemaEntry {
            id: SCHEMA_SUBMIT,
            variant: None,
            fixture: include_str!("schemas/submit.json"),
        },
        SchemaEntry {
            id: SCHEMA_PLAN,
            variant: None,
            fixture: include_str!("schemas/plan.json"),
        },
        SchemaEntry {
            id: SCHEMA_SUBMISSION,
            variant: None,
            fixture: include_str!("schemas/submission.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECEIPT,
            variant: None,
            fixture: include_str!("schemas/receipt.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECEIPTS,
            variant: Some("list"),
            fixture: include_str!("schemas/receipts.json"),
        },
        SchemaEntry {
            id: SCHEMA_VERIFY,
            variant: None,
            fixture: include_str!("schemas/verify.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECONCILE,
            variant: None,
            fixture: include_str!("schemas/reconcile.json"),
        },
        SchemaEntry {
            id: SCHEMA_GRADES,
            variant: None,
            fixture: include_str!("schemas/grades.json"),
        },
        SchemaEntry {
            id: SCHEMA_FILES,
            variant: None,
            fixture: include_str!("schemas/files.json"),
        },
        SchemaEntry {
            id: SCHEMA_MODULES,
            variant: None,
            fixture: include_str!("schemas/modules.json"),
        },
        SchemaEntry {
            id: SCHEMA_DOWNLOAD,
            variant: None,
            fixture: include_str!("schemas/download.json"),
        },
        SchemaEntry {
            id: SCHEMA_ANNOUNCEMENTS,
            variant: None,
            fixture: include_str!("schemas/announcements.json"),
        },
        SchemaEntry {
            id: SCHEMA_ANNOUNCEMENT,
            variant: None,
            fixture: include_str!("schemas/announcement.json"),
        },
        SchemaEntry {
            id: SCHEMA_CALENDAR,
            variant: None,
            fixture: include_str!("schemas/calendar.json"),
        },
        SchemaEntry {
            id: SCHEMA_OPEN,
            variant: None,
            fixture: include_str!("schemas/open.json"),
        },
        SchemaEntry {
            id: SCHEMA_SYNC,
            variant: None,
            fixture: include_str!("schemas/sync.json"),
        },
        SchemaEntry {
            id: SCHEMA_CACHE,
            variant: Some("stats"),
            fixture: include_str!("schemas/cache_stats.json"),
        },
        SchemaEntry {
            id: SCHEMA_ALIAS,
            variant: None,
            fixture: include_str!("schemas/alias.json"),
        },
        SchemaEntry {
            id: SCHEMA_AUTH_STATUS,
            variant: None,
            fixture: include_str!("schemas/auth_status.json"),
        },
        SchemaEntry {
            id: SCHEMA_AUTH_LOGIN,
            variant: None,
            fixture: include_str!("schemas/auth_login.json"),
        },
        SchemaEntry {
            id: SCHEMA_AUTH_LOGOUT,
            variant: None,
            fixture: include_str!("schemas/auth_logout.json"),
        },
        SchemaEntry {
            id: SCHEMA_IDENTITY,
            variant: Some("list"),
            fixture: include_str!("schemas/identity.json"),
        },
        SchemaEntry {
            id: SCHEMA_CONFIG,
            variant: Some("get"),
            fixture: include_str!("schemas/config.json"),
        },
        SchemaEntry {
            id: SCHEMA_DOCTOR,
            variant: None,
            fixture: include_str!("schemas/doctor.json"),
        },
        SchemaEntry {
            id: SCHEMA_VERSION,
            variant: None,
            fixture: include_str!("schemas/version.json"),
        },
        SchemaEntry {
            id: SCHEMA_ERROR,
            variant: None,
            fixture: include_str!("schemas/error.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECEIPTS,
            variant: Some("show"),
            fixture: include_str!("schemas/receipts_show.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECEIPTS,
            variant: Some("export"),
            fixture: include_str!("schemas/receipts_export.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECEIPTS,
            variant: Some("acknowledge"),
            fixture: include_str!("schemas/receipts_acknowledge.json"),
        },
        SchemaEntry {
            id: SCHEMA_CACHE,
            variant: Some("clear"),
            fixture: include_str!("schemas/cache_clear.json"),
        },
        SchemaEntry {
            id: SCHEMA_CACHE,
            variant: Some("path"),
            fixture: include_str!("schemas/cache_path.json"),
        },
        SchemaEntry {
            id: SCHEMA_CONFIG,
            variant: Some("set"),
            fixture: include_str!("schemas/config_set.json"),
        },
        SchemaEntry {
            id: SCHEMA_CONFIG,
            variant: Some("path"),
            fixture: include_str!("schemas/config_path.json"),
        },
        SchemaEntry {
            id: SCHEMA_IDENTITY,
            variant: Some("remove"),
            fixture: include_str!("schemas/identity_remove.json"),
        },
    ]
}

/// The registered schema a command name belongs to.
///
/// The name is the command path with any separator: `auth status`,
/// `auth_status`, and `auth-status` all name `canvas-cli/auth_status@1`.
#[must_use]
pub fn entry_for_command(name: &str) -> Option<&'static SchemaEntry> {
    let wanted = normalize_command(name);
    all_schemas()
        .iter()
        .find(|entry| normalize_command(&crate::output::command_name(entry.id)) == wanted)
}

fn normalize_command(name: &str) -> String {
    name.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Raw-output commands reject `--json` (SPEC §7):
/// `completions`, `auth token --reveal`, `config edit`,
/// `calendar --ics -`, and `receipts export --out -`.
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanFileJson {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

/// The text digests on a `plan@1` document.
///
/// The digests only. `plan@1` never carries the outbound bytes: `sent_sha256`
/// is what the approval binds, and §12.2 step 8 verifies it from the stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanTextJson {
    pub input_sha256: String,
    pub transform: String,
    pub sent_sha256: String,
}

/// The approval audit on a `plan@1` document; `null` before approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanApprovalJson {
    pub channel: String,
    pub at: String,
    pub consumer: Option<String>,
    pub plan_sha256: String,
}

/// A frozen plan (REPORT §3.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanJson {
    pub plan_id: String,
    pub state: String,
    pub consumer: Option<String>,
    pub course_id: String,
    pub course_code: Option<String>,
    pub assignment_id: String,
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
}

/// `plan@1` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            course_id: row.course_id.to_string(),
            course_code: row.payload.course_code.clone(),
            assignment_id: row.assignment_id.to_string(),
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
        }
    }
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
            kind: InputKind::OnlineHtml,
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
