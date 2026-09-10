//! JSON envelope, schema registry, and human renderer base (§7).

#![allow(dead_code, unused_imports)]

mod color;
mod envelope;
mod json_schema;
mod now;
mod registry;
mod render;

pub use color::{ColorMode, resolve_color, should_use_color, should_use_color_with_env};
pub use envelope::{
    Envelope, ErrorResult, Freshness, FreshnessSource, IdentityRef, Outcome, PartialScope,
    Requests, error_envelope, print_human,
};
pub use json_schema::{
    SCHEMA_SCHEMA, command_name, document_for_command, document_for_schema, entry_command,
    list as schema_list,
};
pub use now::{generated_at_now, now_timestamp};
pub use registry::{
    AliasJson, AliasResult, AnnouncementDetailJson, AnnouncementJson, AnnouncementResult,
    AnnouncementsResult, AssignmentGroupJson, CacheClearResult, CachePathResult, CacheStatsResult,
    CacheTableJson, CalendarItemJson, CalendarResult, ConversationAttachmentJson,
    ConversationDetailJson, ConversationMessageJson, ConversationResult, ConversationSummaryJson,
    CourseBaseJson, CourseDetailJson, CourseJson, CourseResult, CoursesResult,
    DiscussionDetailJson, DiscussionReplyJson, DiscussionResult, DiscussionSummaryJson,
    DiscussionsResult, DownloadCourseJson, DownloadFileJson, DownloadResult, DownloadTotalsJson,
    EmbeddedJson, EventJson, ExternalLinkJson, FileEntryJson, FileRefJson, FilesListingJson,
    FilesResult, FollowJson, FollowResult, GradeJson, GradesCourseJson, GradesCourseViewJson,
    GradesResult, GradingPeriodJson, GroupAssignmentJson, GroupRulesJson, GroupSubtotalJson,
    GroupTopicChildJson, HereAccountJson, HereApiJson, HereBrowserJson, HereIdentityJson,
    HereResult, InboxResult, InboxUnreadResult, ModuleEntryJson, ModuleItemJson, ModulesResult,
    NoteJson, NoteResult, PageDetailJson, PageResult, PageSummaryJson, PagesResult,
    ParticipantJson, PeriodJson, PlanApprovalJson, PlanFileJson, PlanJson, PlanResult,
    PlanTextJson, RepliesCoverageJson, SCHEMA_ALIAS, SCHEMA_ANNOUNCEMENT, SCHEMA_ANNOUNCEMENTS,
    SCHEMA_ASSIGNMENT, SCHEMA_ASSIGNMENTS, SCHEMA_AUTH_LOGIN, SCHEMA_AUTH_LOGOUT,
    SCHEMA_AUTH_STATUS, SCHEMA_BRIDGE, SCHEMA_CACHE, SCHEMA_CALENDAR, SCHEMA_CONFIG,
    SCHEMA_CONVERSATION, SCHEMA_COURSE, SCHEMA_COURSES, SCHEMA_DISCUSSION, SCHEMA_DISCUSSIONS,
    SCHEMA_DOCTOR, SCHEMA_DOWNLOAD, SCHEMA_ERROR, SCHEMA_EVENT, SCHEMA_FILES, SCHEMA_FOLLOW,
    SCHEMA_GRADES, SCHEMA_HERE, SCHEMA_IDENTITY, SCHEMA_INBOX, SCHEMA_INBOX_UNREAD, SCHEMA_MODULES,
    SCHEMA_NOTE, SCHEMA_OPEN, SCHEMA_PAGE, SCHEMA_PAGES, SCHEMA_PLAN, SCHEMA_RECEIPT,
    SCHEMA_RECEIPTS, SCHEMA_RECONCILE, SCHEMA_SUBMISSION, SCHEMA_SUBMIT, SCHEMA_SYLLABUS,
    SCHEMA_SYNC, SCHEMA_TODO, SCHEMA_VERIFY, SCHEMA_VERSION, SCHEMA_WATCH, SchemaEntry,
    SubmissionStatusJson, SubmitCandidateJson, SubmitFileJson, SubmitResult, SubmitTextJson,
    SyllabusResult, SyncDatasetJson, SyncResult, TeacherJson, TermJson, WatchResult, WindowJson,
    all_schemas, entry_for_command, entry_for_schema, rejects_json,
};
pub use render::{
    StatusKind, apply_status_style, apply_two_space_padding, format_local_datetime,
    format_local_datetime_at, format_local_instant, format_relative_suffix,
    format_relative_suffix_at, new_table, paint, status_label, style_dim, style_due_soon,
    style_missing, style_overdue, style_submitted,
};
