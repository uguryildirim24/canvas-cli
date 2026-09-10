//! JSON envelope, schema registry, and human renderer base (§7).

#![allow(dead_code, unused_imports)]

mod color;
mod envelope;
mod now;
mod registry;
mod render;

pub use color::{ColorMode, resolve_color, should_use_color, should_use_color_with_env};
pub use envelope::{
    Envelope, ErrorResult, Freshness, FreshnessSource, IdentityRef, Outcome, PartialScope,
    Requests, error_envelope, print_human,
};
pub use now::{generated_at_now, now_timestamp};
pub use registry::{
    AliasJson, AliasResult, AssignmentGroupJson, CacheClearResult, CachePathResult,
    CacheStatsResult, CacheTableJson, CourseBaseJson, CourseDetailJson, CourseJson, CourseResult,
    CoursesResult, DownloadCourseJson, DownloadFileJson, DownloadResult, DownloadTotalsJson,
    FileEntryJson, FilesListingJson, FilesResult, GradeJson, GradesCourseJson,
    GradesCourseViewJson, GradesResult, GradingPeriodJson, GroupAssignmentJson, GroupRulesJson,
    GroupSubtotalJson, ModuleEntryJson, ModuleItemJson, ModulesResult, PeriodJson,
    PlanApprovalJson, PlanFileJson, PlanJson, PlanResult, PlanTextJson, SCHEMA_ALIAS,
    SCHEMA_ANNOUNCEMENT, SCHEMA_ANNOUNCEMENTS, SCHEMA_ASSIGNMENT, SCHEMA_ASSIGNMENTS,
    SCHEMA_AUTH_LOGIN, SCHEMA_AUTH_LOGOUT, SCHEMA_AUTH_STATUS, SCHEMA_CACHE, SCHEMA_CALENDAR,
    SCHEMA_CONFIG, SCHEMA_COURSE, SCHEMA_COURSES, SCHEMA_DOCTOR, SCHEMA_DOWNLOAD, SCHEMA_ERROR,
    SCHEMA_FILES, SCHEMA_GRADES, SCHEMA_IDENTITY, SCHEMA_MODULES, SCHEMA_OPEN, SCHEMA_PLAN,
    SCHEMA_RECEIPT, SCHEMA_RECEIPTS, SCHEMA_RECONCILE, SCHEMA_SUBMISSION, SCHEMA_SUBMIT,
    SCHEMA_SYNC, SCHEMA_TODO, SCHEMA_VERIFY, SCHEMA_VERSION, SchemaEntry, SubmissionStatusJson,
    SubmitCandidateJson, SubmitFileJson, SubmitResult, SubmitTextJson, SyncDatasetJson, SyncResult,
    TeacherJson, TermJson, all_schemas, rejects_json,
};
pub use render::{
    StatusKind, apply_status_style, apply_two_space_padding, format_local_datetime,
    format_local_datetime_at, format_local_instant, format_relative_suffix,
    format_relative_suffix_at, new_table, paint, status_label, style_dim, style_due_soon,
    style_missing, style_overdue, style_submitted,
};
