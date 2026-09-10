//! The tool catalog of `canvas mcp` (REPORT §3.2).
//!
//! Every tool preserves the arguments of the v1 command behind it and returns
//! that command's §7 result. What is absent is absent by design: credentials,
//! token reveal, identity administration, arbitrary HTTP or shell, `--yes`,
//! cache clearing, `download --force`, and any browser action.
//!
//! Annotations describe effects, not command classes, and they are
//! documentation only. Enforcement lives in the plan and approval core.

use std::borrow::Cow;
use std::sync::Arc;

use rmcp::model::{JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::cli::AssignmentBucket;
use crate::commands::{
    Globals, announcement, announcements, assignment, assignments, calendar, course, courses,
    download, files, grades, handled::Handled, modules, open, receipts, submission, sync, todo,
};
use crate::output::{
    SCHEMA_ANNOUNCEMENT, SCHEMA_ANNOUNCEMENTS, SCHEMA_ASSIGNMENT, SCHEMA_ASSIGNMENTS,
    SCHEMA_CALENDAR, SCHEMA_COURSE, SCHEMA_COURSES, SCHEMA_DOWNLOAD, SCHEMA_FILES, SCHEMA_GRADES,
    SCHEMA_MODULES, SCHEMA_OPEN, SCHEMA_RECEIPTS, SCHEMA_RECONCILE, SCHEMA_SUBMISSION, SCHEMA_SYNC,
    SCHEMA_TODO,
};

/// What a tool does to its environment (§3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// An authorized read: nothing changes.
    Read,
    /// Local organization: the cache, a manifest, or a plan changes.
    Organize,
    /// A durable local decision that retires evidence.
    Retire,
}

impl Effect {
    fn annotations(self, title: &str, idempotent: bool, open_world: bool) -> ToolAnnotations {
        let mut annotations = ToolAnnotations::new();
        annotations.title = Some(title.to_owned());
        annotations.read_only_hint = Some(self == Self::Read);
        // Nothing in this catalog performs a destructive update.
        annotations.destructive_hint = Some(false);
        annotations.idempotent_hint = Some(idempotent);
        annotations.open_world_hint = Some(open_world);
        annotations
    }
}

/// One tool: its name, its effect, and the command core behind it.
pub struct ToolSpec {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// The `canvas-cli/<name>@<n>` schema of the result it returns.
    pub schema: &'static str,
    pub effect: Effect,
    pub idempotent: bool,
    pub open_world: bool,
    input_schema: fn() -> JsonObject,
}

impl ToolSpec {
    /// The MCP tool definition, with the §7 envelope as its output schema.
    #[must_use]
    pub fn tool(&self) -> Tool {
        Tool::new(
            Cow::Borrowed(self.name),
            Cow::Borrowed(self.description),
            Arc::new((self.input_schema)()),
        )
        .with_title(self.title)
        .with_raw_output_schema(Arc::new(output_schema(self.schema)))
        .with_annotations(self.effect.annotations(
            self.title,
            self.idempotent,
            self.open_world,
        ))
    }
}

/// The output schema of a tool: the success envelope or the domain error.
///
/// Both shapes are admitted, because a domain failure keeps the envelope
/// (§3.2). The document comes from `canvas schema <command>`, so a tool and
/// the CLI cannot describe their output differently.
fn output_schema(schema_id: &str) -> JsonObject {
    crate::output::document_for_schema(schema_id)
        .and_then(|mut document| document.get_mut("envelope").map(std::mem::take))
        .and_then(|envelope| match envelope {
            Value::Object(object) => Some(object),
            _ => None,
        })
        .unwrap_or_default()
}

fn schema_of<T: JsonSchema>() -> JsonObject {
    let mut settings = schemars::generate::SchemaSettings::draft2020_12();
    settings.inline_subschemas = true;
    settings.meta_schema = None;
    match schemars::SchemaGenerator::new(settings)
        .into_root_schema_for::<T>()
        .to_value()
    {
        Value::Object(object) => object,
        _ => JsonObject::new(),
    }
}

// ----------------------------------------------------------------- arguments

/// `<course>` accepts a numeric id, an alias, a URL, or a code substring (§6).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoursesListArgs {
    /// Include completed and invited courses, not only the active ones.
    #[serde(default)]
    pub all: bool,
    /// Keep only courses whose term name contains this text.
    #[serde(default)]
    pub term: Option<String>,
    /// Keep only favorite courses.
    #[serde(default)]
    pub favorites: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CourseGetArgs {
    /// Course id, alias, URL, or a substring of its code or name.
    pub course: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TodoListArgs {
    /// Days ahead to include. The default window is 14 days.
    #[serde(default)]
    pub days: Option<u32>,
    /// Include items that the default window hides.
    #[serde(default)]
    pub all: bool,
    /// Only work Canvas reports as missing.
    #[serde(default)]
    pub missing: bool,
    /// Only one course.
    #[serde(default)]
    pub course: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignmentsListArgs {
    pub course: String,
    /// Which assignments to keep. The default is `open`.
    #[serde(default)]
    pub bucket: Option<AssignmentBucket>,
    /// Substring of the assignment name.
    #[serde(default)]
    pub search: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignmentGetArgs {
    /// Course id, alias, code, or a full Canvas assignment URL.
    pub course: String,
    /// Assignment id or a substring of its name. Omit it when `course` is a URL.
    #[serde(default)]
    pub assignment: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GradesGetArgs {
    /// One course, or every course when absent.
    #[serde(default)]
    pub course: Option<String>,
    /// `current`, `all`, or a grading-period id.
    #[serde(default)]
    pub period: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesListArgs {
    pub course: String,
    /// Group the files by folder.
    #[serde(default)]
    pub tree: bool,
    /// Substring of the file name.
    #[serde(default)]
    pub search: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModulesListArgs {
    pub course: String,
    /// Include the items of each module.
    #[serde(default)]
    pub items: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnouncementsListArgs {
    /// One course, or every active course when absent.
    #[serde(default)]
    pub course: Option<String>,
    /// How far back to look, for example `7d` or `48h`. The default is 14 days.
    #[serde(default)]
    pub since: Option<String>,
    /// Only announcements this identity has not read. Reading one here never
    /// marks it read.
    #[serde(default)]
    pub unread: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnouncementGetArgs {
    /// Course id, alias, code, or a full Canvas announcement URL.
    pub course: String,
    /// Announcement id. Omit it when `course` is a URL. A bare id is refused.
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarListArgs {
    /// Days ahead to include. The default window is 14 days.
    #[serde(default)]
    pub days: Option<u32>,
    /// Only one course.
    #[serde(default)]
    pub course: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubmissionGetArgs {
    pub course: String,
    pub assignment: String,
    /// Include every previous attempt.
    #[serde(default)]
    pub history: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReceiptsListArgs {
    #[serde(default)]
    pub course: Option<String>,
    /// Receipt or journal state to keep.
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReceiptsShowArgs {
    /// Receipt id or journal id.
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SyncRunArgs {
    /// Also refresh folders, files, modules, and calendar events.
    #[serde(default)]
    pub full: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DownloadArgs {
    /// One course, or every active course with `all_courses`.
    #[serde(default)]
    pub course: Option<String>,
    /// Every active course.
    #[serde(default)]
    pub all_courses: bool,
    /// Only files in modules whose name contains this text.
    #[serde(default)]
    pub module: Option<String>,
    /// Only these file ids.
    #[serde(default)]
    pub files: Vec<i64>,
    /// Parallel transfers. The configured value is the default.
    #[serde(default)]
    pub jobs: Option<u32>,
    /// Verify the bytes of files that are already present.
    #[serde(default)]
    pub verify: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReconcileArgs {
    /// The journal to resolve.
    pub journal_id: String,
    /// Retire the evidence and record that nothing was submitted. This is the
    /// only argument in the catalog that retires evidence, and §12.2 still
    /// refuses it while an attempt is visible or the journal is too young.
    #[serde(default)]
    pub assume_not_submitted: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcknowledgeArgs {
    /// The journal whose unknown outcome is accepted.
    pub journal_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenUrlArgs {
    /// A course, a Canvas URL, or `assignment`/`file`/`announcement` target.
    pub target: String,
}

// ------------------------------------------------------------------ dispatch

/// Every tool this server exposes, in a stable order.
#[must_use]
pub fn specs() -> &'static [ToolSpec] {
    &[
        ToolSpec {
            name: "courses.list",
            title: "List courses",
            description: "List the courses of the bound identity, with the grades Canvas reports.",
            schema: SCHEMA_COURSES,
            effect: Effect::Read,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<CoursesListArgs>,
        },
        ToolSpec {
            name: "course.get",
            title: "Show one course",
            description: "Show one course: term, teachers, syllabus, and reported scores.",
            schema: SCHEMA_COURSE,
            effect: Effect::Read,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<CourseGetArgs>,
        },
        ToolSpec {
            name: "todo.list",
            title: "List what is due",
            description: "What is due and what Canvas reports as missing, in one merged list.",
            schema: SCHEMA_TODO,
            effect: Effect::Read,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<TodoListArgs>,
        },
        ToolSpec {
            name: "assignments.list",
            title: "List assignments",
            description: "List a course's assignments with your submission state.",
            schema: SCHEMA_ASSIGNMENTS,
            effect: Effect::Read,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<AssignmentsListArgs>,
        },
        ToolSpec {
            name: "assignment.get",
            title: "Show one assignment",
            description: "Show one assignment: prompt as Markdown, dates, rubric, and submission.",
            schema: SCHEMA_ASSIGNMENT,
            effect: Effect::Read,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<AssignmentGetArgs>,
        },
        ToolSpec {
            name: "grades.get",
            title: "Show grades",
            description: "Scores as Canvas reports them, per course or for one course's groups.",
            schema: SCHEMA_GRADES,
            effect: Effect::Read,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<GradesGetArgs>,
        },
        ToolSpec {
            name: "files.list",
            title: "List course files",
            description: "List a course's files, flat or grouped by folder.",
            schema: SCHEMA_FILES,
            effect: Effect::Read,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<FilesListArgs>,
        },
        ToolSpec {
            name: "modules.list",
            title: "List modules",
            description: "List a course's modules, with their items when asked.",
            schema: SCHEMA_MODULES,
            effect: Effect::Read,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<ModulesListArgs>,
        },
        ToolSpec {
            name: "announcements.list",
            title: "List announcements",
            description: "Recent announcements across courses. Reading never marks one read.",
            schema: SCHEMA_ANNOUNCEMENTS,
            effect: Effect::Read,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<AnnouncementsListArgs>,
        },
        ToolSpec {
            name: "announcement.get",
            title: "Show one announcement",
            description: "Show one announcement's message as Markdown. It stays unread.",
            schema: SCHEMA_ANNOUNCEMENT,
            effect: Effect::Read,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<AnnouncementGetArgs>,
        },
        ToolSpec {
            name: "calendar.list",
            title: "List calendar items",
            description: "Deadlines and calendar events in one window, in the identity time zone.",
            schema: SCHEMA_CALENDAR,
            effect: Effect::Read,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<CalendarListArgs>,
        },
        ToolSpec {
            name: "submission.get",
            title: "Show a submission",
            description: "Your submission for one assignment, with its attempts when asked.",
            schema: SCHEMA_SUBMISSION,
            effect: Effect::Read,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<SubmissionGetArgs>,
        },
        ToolSpec {
            name: "receipts.list",
            title: "List receipts",
            description: "Local submission receipts and unresolved journals. Local only.",
            schema: SCHEMA_RECEIPTS,
            effect: Effect::Read,
            idempotent: true,
            open_world: false,
            input_schema: schema_of::<ReceiptsListArgs>,
        },
        ToolSpec {
            name: "receipts.show",
            title: "Show one receipt",
            description: "One receipt or journal in full, as the local record holds it.",
            schema: SCHEMA_RECEIPTS,
            effect: Effect::Read,
            idempotent: true,
            open_world: false,
            input_schema: schema_of::<ReceiptsShowArgs>,
        },
        ToolSpec {
            name: "sync.run",
            title: "Refresh the cache",
            description: "Refresh the cached datasets. It writes the cache, never Canvas.",
            schema: SCHEMA_SYNC,
            effect: Effect::Organize,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<SyncRunArgs>,
        },
        ToolSpec {
            name: "download.plan",
            title: "Plan a download",
            description: "What a download would transfer, per file. It writes nothing.",
            schema: SCHEMA_DOWNLOAD,
            effect: Effect::Read,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<DownloadArgs>,
        },
        ToolSpec {
            name: "download.run",
            title: "Download course files",
            description: "Download course files into the configured destination. \
                          It never overwrites a file it does not own.",
            schema: SCHEMA_DOWNLOAD,
            effect: Effect::Organize,
            idempotent: false,
            open_world: true,
            input_schema: schema_of::<DownloadArgs>,
        },
        ToolSpec {
            name: "submission.reconcile",
            title: "Reconcile a journal",
            description: "Resolve a journal an interrupted submit left behind. \
                          Ordinary reconciliation never posts to Canvas.",
            schema: SCHEMA_RECONCILE,
            effect: Effect::Retire,
            idempotent: true,
            open_world: true,
            input_schema: schema_of::<ReconcileArgs>,
        },
        ToolSpec {
            name: "receipts.acknowledge",
            title: "Acknowledge an unknown outcome",
            description: "Accept a journal whose outcome stays unknown. Local, and final.",
            schema: SCHEMA_RECEIPTS,
            effect: Effect::Retire,
            idempotent: true,
            open_world: false,
            input_schema: schema_of::<AcknowledgeArgs>,
        },
        ToolSpec {
            name: "open.url",
            title: "Resolve a Canvas URL",
            description: "Resolve a target to its canonical Canvas URL. It never opens a browser.",
            schema: SCHEMA_OPEN,
            effect: Effect::Read,
            idempotent: true,
            open_world: false,
            input_schema: schema_of::<OpenUrlArgs>,
        },
    ]
}

/// The spec of one tool name.
#[must_use]
pub fn spec(name: &str) -> Option<&'static ToolSpec> {
    specs().iter().find(|spec| spec.name == name)
}

/// Parse `arguments` for a tool, rejecting anything its schema does not name.
fn parse<T: for<'de> Deserialize<'de>>(arguments: Option<JsonObject>) -> Result<T, String> {
    let value = Value::Object(arguments.unwrap_or_default());
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// Run one tool through the command core behind it.
///
/// `Err` is an argument failure: the caller turns it into a JSON-RPC error,
/// because the tool never ran. Every outcome the command itself produces —
/// including a refusal — comes back as `Ok(Handled)` with its envelope.
pub async fn dispatch(
    globals: &Globals,
    name: &str,
    arguments: Option<JsonObject>,
) -> Result<Handled, String> {
    Ok(match name {
        "courses.list" => {
            let args: CoursesListArgs = parse(arguments)?;
            courses::handle(globals, args.all, args.term, args.favorites).await
        }
        "course.get" => {
            let args: CourseGetArgs = parse(arguments)?;
            course::handle(globals, args.course).await
        }
        "todo.list" => {
            let args: TodoListArgs = parse(arguments)?;
            todo::handle(globals, args.days, args.all, args.missing, args.course).await
        }
        "assignments.list" => {
            let args: AssignmentsListArgs = parse(arguments)?;
            assignments::handle(globals, args.course, args.bucket, args.search).await
        }
        "assignment.get" => {
            let args: AssignmentGetArgs = parse(arguments)?;
            assignment::handle(globals, args.course, args.assignment).await
        }
        "grades.get" => {
            let args: GradesGetArgs = parse(arguments)?;
            grades::handle(globals, args.course, args.period).await
        }
        "files.list" => {
            let args: FilesListArgs = parse(arguments)?;
            files::handle(globals, args.course, args.tree, args.search).await
        }
        "modules.list" => {
            let args: ModulesListArgs = parse(arguments)?;
            modules::handle(globals, args.course, args.items).await
        }
        "announcements.list" => {
            let args: AnnouncementsListArgs = parse(arguments)?;
            announcements::handle(globals, args.course, args.since, args.unread).await
        }
        "announcement.get" => {
            let args: AnnouncementGetArgs = parse(arguments)?;
            announcement::handle(globals, args.course, args.id).await
        }
        "calendar.list" => {
            let args: CalendarListArgs = parse(arguments)?;
            calendar::handle(
                globals,
                calendar::CalendarArgs {
                    days: args.days,
                    course: args.course,
                    // `--ics` writes a file and `--alarm` only shapes that
                    // file, so neither belongs on an agent surface.
                    ics: None,
                    alarm: None,
                },
            )
            .await
        }
        "submission.get" => {
            let args: SubmissionGetArgs = parse(arguments)?;
            submission::handle(
                globals,
                submission::SubmissionCmd::Show {
                    course: args.course,
                    assignment: Some(args.assignment),
                    history: args.history,
                },
            )
            .await
        }
        "receipts.list" => {
            let args: ReceiptsListArgs = parse(arguments)?;
            receipts::handle(
                globals,
                receipts::ReceiptsCmd::List {
                    course: args.course,
                    state: args.state,
                },
            )
        }
        "receipts.show" => {
            let args: ReceiptsShowArgs = parse(arguments)?;
            receipts::handle(globals, receipts::ReceiptsCmd::Show { id: args.id })
        }
        "sync.run" => {
            let args: SyncRunArgs = parse(arguments)?;
            sync::handle(globals, args.full).await
        }
        "download.plan" | "download.run" => {
            let args: DownloadArgs = parse(arguments)?;
            download::handle(
                globals,
                download::DownloadArgs {
                    course: args.course,
                    all_courses: args.all_courses,
                    // The configured destination only: an agent cannot choose
                    // where bytes land, and it cannot overwrite with --force.
                    dest: None,
                    module: args.module,
                    files: args.files,
                    jobs: args.jobs,
                    dry_run: name == "download.plan",
                    force: false,
                    verify: args.verify,
                },
            )
            .await
        }
        "submission.reconcile" => {
            let args: ReconcileArgs = parse(arguments)?;
            submission::handle(
                globals,
                submission::SubmissionCmd::Reconcile {
                    journal_id: args.journal_id,
                    assume_not_submitted: args.assume_not_submitted,
                },
            )
            .await
        }
        "receipts.acknowledge" => {
            let args: AcknowledgeArgs = parse(arguments)?;
            receipts::handle(
                globals,
                receipts::ReceiptsCmd::Acknowledge {
                    journal_id: args.journal_id,
                },
            )
        }
        "open.url" => {
            let args: OpenUrlArgs = parse(arguments)?;
            open::handle(globals, None, Some(args.target), open::Launch::No).await
        }
        other => return Err(format!("unknown tool {other}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REPORT §3.2 names the catalog exactly. This test is the allowlist.
    #[test]
    fn the_catalog_is_the_report_catalog() {
        let expected = [
            "courses.list",
            "course.get",
            "todo.list",
            "assignments.list",
            "assignment.get",
            "grades.get",
            "files.list",
            "modules.list",
            "announcements.list",
            "announcement.get",
            "calendar.list",
            "submission.get",
            "receipts.list",
            "receipts.show",
            "sync.run",
            "download.plan",
            "download.run",
            "submission.reconcile",
            "receipts.acknowledge",
            "open.url",
        ];
        let names: Vec<&str> = specs().iter().map(|spec| spec.name).collect();
        assert_eq!(names, expected);
    }

    /// Absent by design (§3.2): credentials, token reveal, identity
    /// administration, arbitrary HTTP or shell, cache clearing, and any
    /// browser action.
    #[test]
    fn no_tool_reaches_a_forbidden_surface() {
        for spec in specs() {
            let name = spec.name;
            for forbidden in [
                "auth",
                "token",
                "identity",
                "credential",
                "cache",
                "config",
                "http",
                "shell",
                "exec",
                "browser",
                "launch",
                "quiz",
            ] {
                assert!(
                    !name.contains(forbidden),
                    "{name} names the forbidden surface {forbidden}"
                );
            }
        }
    }

    /// No tool takes `--yes`, a destination, a force flag, or a raw-output
    /// switch, and `assume_not_submitted` is the only evidence-retiring
    /// argument in the catalog.
    #[test]
    fn no_tool_takes_a_forbidden_argument() {
        let mut retiring = Vec::new();
        for spec in specs() {
            let schema = (spec.input_schema)();
            let properties = schema
                .get("properties")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            for name in properties.keys() {
                for forbidden in [
                    "yes", "force", "dest", "out", "ics", "reveal", "token", "host", "replace",
                    "alarm", "path",
                ] {
                    assert!(
                        name != forbidden,
                        "{}.{name} exposes the forbidden argument {forbidden}",
                        spec.name
                    );
                }
                if name == "assume_not_submitted" {
                    retiring.push(spec.name);
                }
            }
        }
        assert_eq!(retiring, ["submission.reconcile"]);
    }

    /// Hints describe effects (§3.2): only a pure read is read-only, and
    /// nothing is destructive.
    #[test]
    fn annotations_describe_effects() {
        for spec in specs() {
            let tool = spec.tool();
            let annotations = tool.annotations.expect("annotations");
            assert_eq!(
                annotations.read_only_hint,
                Some(spec.effect == Effect::Read),
                "{}",
                spec.name
            );
            assert_eq!(annotations.destructive_hint, Some(false), "{}", spec.name);
            assert_eq!(
                annotations.idempotent_hint,
                Some(spec.idempotent),
                "{}",
                spec.name
            );
        }
        // The write and organize tools are not advertised as pure reads.
        for name in [
            "sync.run",
            "download.run",
            "submission.reconcile",
            "receipts.acknowledge",
        ] {
            let spec = spec(name).expect(name);
            assert_ne!(spec.effect, Effect::Read, "{name}");
        }
    }

    /// Every tool's output schema admits the success envelope and the domain
    /// error, because a domain failure keeps the envelope (§3.2).
    #[test]
    fn every_output_schema_admits_both_shapes() {
        for spec in specs() {
            let tool = spec.tool();
            let schema = tool.output_schema.expect("output schema");
            let branches = schema["oneOf"].as_array().expect("oneOf");
            assert_eq!(branches.len(), 2, "{}", spec.name);
            assert_eq!(
                branches[0]["properties"]["schema"]["const"], spec.schema,
                "{}",
                spec.name
            );
            assert_eq!(
                branches[1]["properties"]["schema"]["const"],
                crate::output::SCHEMA_ERROR,
                "{}",
                spec.name
            );
        }
    }

    #[test]
    fn arguments_that_the_schema_does_not_name_are_refused() {
        let refused = parse::<DownloadArgs>(Some(
            serde_json::json!({ "force": true })
                .as_object()
                .cloned()
                .expect("object"),
        ));
        assert!(refused.is_err(), "force must not deserialize");
        let accepted: DownloadArgs = parse(None).expect("every field has a default");
        assert!(!accepted.all_courses && !accepted.verify);
    }

    #[test]
    fn reconcile_does_not_assume_anything_by_default() {
        let args: ReconcileArgs = parse(Some(
            serde_json::json!({ "journal_id": "journal-1" })
                .as_object()
                .cloned()
                .expect("object"),
        ))
        .expect("journal id only");
        assert!(!args.assume_not_submitted);
    }
}
