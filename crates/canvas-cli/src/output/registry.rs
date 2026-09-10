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
pub const SCHEMA_VERSION: &str = "canvas-cli/version@1";
pub const SCHEMA_ERROR: &str = "canvas-cli/error@1";

/// One registered schema and its example `result` fixture JSON.
#[derive(Debug, Clone, Copy)]
pub struct SchemaEntry {
    pub id: &'static str,
    pub fixture: &'static str,
}

/// Every Appendix D v1 schema with a fixture payload.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn all_schemas() -> &'static [SchemaEntry] {
    &[
        SchemaEntry {
            id: SCHEMA_COURSES,
            fixture: include_str!("schemas/courses.json"),
        },
        SchemaEntry {
            id: SCHEMA_COURSE,
            fixture: include_str!("schemas/course.json"),
        },
        SchemaEntry {
            id: SCHEMA_TODO,
            fixture: include_str!("schemas/todo.json"),
        },
        SchemaEntry {
            id: SCHEMA_ASSIGNMENTS,
            fixture: include_str!("schemas/assignments.json"),
        },
        SchemaEntry {
            id: SCHEMA_ASSIGNMENT,
            fixture: include_str!("schemas/assignment.json"),
        },
        SchemaEntry {
            id: SCHEMA_SUBMIT,
            fixture: include_str!("schemas/submit.json"),
        },
        SchemaEntry {
            id: SCHEMA_SUBMISSION,
            fixture: include_str!("schemas/submission.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECEIPT,
            fixture: include_str!("schemas/receipt.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECEIPTS,
            fixture: include_str!("schemas/receipts.json"),
        },
        SchemaEntry {
            id: SCHEMA_VERIFY,
            fixture: include_str!("schemas/verify.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECONCILE,
            fixture: include_str!("schemas/reconcile.json"),
        },
        SchemaEntry {
            id: SCHEMA_GRADES,
            fixture: include_str!("schemas/grades.json"),
        },
        SchemaEntry {
            id: SCHEMA_FILES,
            fixture: include_str!("schemas/files.json"),
        },
        SchemaEntry {
            id: SCHEMA_MODULES,
            fixture: include_str!("schemas/modules.json"),
        },
        SchemaEntry {
            id: SCHEMA_DOWNLOAD,
            fixture: include_str!("schemas/download.json"),
        },
        SchemaEntry {
            id: SCHEMA_ANNOUNCEMENTS,
            fixture: include_str!("schemas/announcements.json"),
        },
        SchemaEntry {
            id: SCHEMA_ANNOUNCEMENT,
            fixture: include_str!("schemas/announcement.json"),
        },
        SchemaEntry {
            id: SCHEMA_CALENDAR,
            fixture: include_str!("schemas/calendar.json"),
        },
        SchemaEntry {
            id: SCHEMA_OPEN,
            fixture: include_str!("schemas/open.json"),
        },
        SchemaEntry {
            id: SCHEMA_SYNC,
            fixture: include_str!("schemas/sync.json"),
        },
        SchemaEntry {
            id: SCHEMA_CACHE,
            fixture: include_str!("schemas/cache.json"),
        },
        SchemaEntry {
            id: SCHEMA_ALIAS,
            fixture: include_str!("schemas/alias.json"),
        },
        SchemaEntry {
            id: SCHEMA_AUTH_STATUS,
            fixture: include_str!("schemas/auth_status.json"),
        },
        SchemaEntry {
            id: SCHEMA_AUTH_LOGIN,
            fixture: include_str!("schemas/auth_login.json"),
        },
        SchemaEntry {
            id: SCHEMA_AUTH_LOGOUT,
            fixture: include_str!("schemas/auth_logout.json"),
        },
        SchemaEntry {
            id: SCHEMA_IDENTITY,
            fixture: include_str!("schemas/identity.json"),
        },
        SchemaEntry {
            id: SCHEMA_CONFIG,
            fixture: include_str!("schemas/config.json"),
        },
        SchemaEntry {
            id: SCHEMA_DOCTOR,
            fixture: include_str!("schemas/doctor.json"),
        },
        SchemaEntry {
            id: SCHEMA_VERSION,
            fixture: include_str!("schemas/version.json"),
        },
        SchemaEntry {
            id: SCHEMA_ERROR,
            fixture: include_str!("schemas/error.json"),
        },
    ]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TermJson {
    pub id: Option<String>,
    pub name: Option<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeriodJson {
    pub mode: String,
    pub id: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradeJson {
    pub current_score: Option<f64>,
    pub current_grade: Option<String>,
    pub final_score: Option<f64>,
    pub final_grade: Option<String>,
    pub period: PeriodJson,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeacherJson {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoursesResult {
    pub courses: Vec<CourseJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CourseResult {
    pub course: CourseDetailJson,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AliasJson {
    pub name: String,
    pub course_id: String,
    pub course_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AliasResult {
    pub aliases: Vec<AliasJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncResult {
    pub datasets: Vec<SyncDatasetJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheTableJson {
    pub name: String,
    pub rows: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheStatsResult {
    pub path: String,
    pub size_bytes: u64,
    pub tables: Vec<CacheTableJson>,
    pub datasets: Vec<Freshness>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheClearResult {
    pub cleared: bool,
    pub rows_deleted: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachePathResult {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesListingJson {
    pub available: bool,
    pub http_status: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesResult {
    pub course_id: String,
    pub listing: FilesListingJson,
    pub files: Vec<FileEntryJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleEntryJson {
    pub id: String,
    pub name: String,
    pub position: i64,
    pub state: Option<String>,
    pub items_count: Option<u64>,
    pub items_complete: bool,
    pub items: Vec<ModuleItemJson>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModulesResult {
    pub course_id: String,
    pub modules: Vec<ModuleEntryJson>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::envelope::{Envelope, IdentityRef};

    #[test]
    fn every_registered_schema_has_a_parseable_fixture() {
        use crate::output::now::with_canvas_now;
        with_canvas_now("2026-09-09T17:05:12Z", || {
            let mut rendered = Vec::new();
            let mut ids = std::collections::HashSet::new();
            for entry in all_schemas() {
                assert!(ids.insert(entry.id), "duplicate schema {}", entry.id);
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
