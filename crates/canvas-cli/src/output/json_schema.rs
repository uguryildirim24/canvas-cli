//! JSON Schema documents for the `--json` output contract (`schema@1`).
//!
//! `canvas schema <command>` prints one of these. Nothing here is written by
//! hand: the envelope and every typed `result` come from the Rust types
//! through `schemars`, and a command whose `result` is still built as a JSON
//! value is described from its registry fixture. A command's schema therefore
//! cannot drift from what the command prints.

use schemars::{JsonSchema, Schema, SchemaGenerator, generate::SchemaSettings};
use serde_json::{Map, Value, json};

use crate::output::envelope::{Envelope, ErrorResult};
use crate::output::registry::{
    self, AliasResult, AnnouncementResult, AnnouncementsResult, CacheStatsResult, CalendarResult,
    CourseResult, CoursesResult, DownloadResult, EventJson, FilesResult, FollowResult,
    GradesResult, HereResult, ModulesResult, NoteResult, PlanResult, SCHEMA_ALIAS,
    SCHEMA_ANNOUNCEMENT, SCHEMA_ANNOUNCEMENTS, SCHEMA_CACHE, SCHEMA_CALENDAR, SCHEMA_COURSE,
    SCHEMA_COURSES, SCHEMA_DOWNLOAD, SCHEMA_ERROR, SCHEMA_EVENT, SCHEMA_FILES, SCHEMA_FOLLOW,
    SCHEMA_GRADES, SCHEMA_HERE, SCHEMA_MODULES, SCHEMA_NOTE, SCHEMA_PLAN, SCHEMA_SUBMIT,
    SCHEMA_SYNC, SCHEMA_WATCH, SchemaEntry, SubmitResult, SyncResult, WatchResult,
};

/// The contract version of the document `canvas schema` prints.
pub const SCHEMA_SCHEMA: &str = "canvas-cli/schema@1";

/// JSON Schema dialect the documents declare.
const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// A generator that inlines every subschema, so one document stands alone.
fn generator() -> SchemaGenerator {
    let mut settings = SchemaSettings::draft2020_12();
    settings.inline_subschemas = true;
    settings.meta_schema = None;
    SchemaGenerator::new(settings)
}

fn schema_of<T: JsonSchema>() -> Value {
    let mut schema = generator().into_root_schema_for::<T>().to_value();
    require_every_property(&mut schema);
    schema
}

/// Every field Appendix D defines is always present; `null` carries "unknown"
/// (§7). A generated schema marks only non-optional fields required, so this
/// walks the document and requires them all.
fn require_every_property(schema: &mut Value) {
    match schema {
        Value::Object(object) => {
            if let Some(properties) = object.get("properties").and_then(Value::as_object) {
                let names: Vec<Value> = properties.keys().map(|key| json!(key)).collect();
                if !names.is_empty() {
                    object.insert("required".into(), Value::Array(names));
                }
            }
            for value in object.values_mut() {
                require_every_property(value);
            }
        }
        Value::Array(items) => {
            for value in items {
                require_every_property(value);
            }
        }
        _ => {}
    }
}

/// Describe a `result` payload that is still assembled as a JSON value.
///
/// The fixture in the registry is the example Appendix D documents, so it is
/// also the only machine-readable description of that payload's shape.
fn schema_of_fixture(fixture: &str) -> Value {
    let value: Value = serde_json::from_str(fixture).unwrap_or(Value::Null);
    let mut schema = generator()
        .into_root_schema_for_value(&value)
        .map_or_else(|_| json!({}), Schema::to_value);
    require_every_property(&mut schema);
    schema
}

/// The schema of one command's `result`, from its type when it has one.
fn result_schema(entry: &SchemaEntry) -> (Value, &'static str) {
    let typed = match entry.id {
        SCHEMA_COURSES => Some(schema_of::<CoursesResult>()),
        SCHEMA_COURSE => Some(schema_of::<CourseResult>()),
        SCHEMA_ALIAS => Some(schema_of::<AliasResult>()),
        SCHEMA_SYNC => Some(schema_of::<SyncResult>()),
        SCHEMA_CACHE => Some(schema_of::<CacheStatsResult>()),
        SCHEMA_SUBMIT => Some(schema_of::<SubmitResult>()),
        SCHEMA_PLAN => Some(schema_of::<PlanResult>()),
        SCHEMA_HERE => Some(schema_of::<HereResult>()),
        SCHEMA_NOTE => Some(schema_of::<NoteResult>()),
        SCHEMA_FOLLOW => Some(schema_of::<FollowResult>()),
        SCHEMA_FILES => Some(schema_of::<FilesResult>()),
        SCHEMA_MODULES => Some(schema_of::<ModulesResult>()),
        SCHEMA_GRADES => Some(schema_of::<GradesResult>()),
        SCHEMA_DOWNLOAD => Some(schema_of::<DownloadResult>()),
        SCHEMA_ANNOUNCEMENTS => Some(schema_of::<AnnouncementsResult>()),
        SCHEMA_ANNOUNCEMENT => Some(schema_of::<AnnouncementResult>()),
        SCHEMA_CALENDAR => Some(schema_of::<CalendarResult>()),
        SCHEMA_ERROR => Some(schema_of::<ErrorResult>()),
        SCHEMA_EVENT => Some(schema_of::<EventJson>()),
        SCHEMA_WATCH => Some(schema_of::<WatchResult>()),
        _ => None,
    };
    match typed {
        Some(schema) => (schema, "result type"),
        None => (schema_of_fixture(entry.fixture), "registry fixture"),
    }
}

/// The envelope schema with `schema` pinned and `result` replaced.
fn envelope_schema(schema_id: &str, result: Value) -> Value {
    let mut envelope = schema_of::<Envelope<Value>>();
    if let Some(object) = envelope.as_object_mut() {
        object.insert("title".into(), json!(schema_id));
        if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
            properties.insert("result".into(), result);
            pin_schema_field(properties, schema_id);
        }
    }
    envelope
}

/// Pin the self-describing `schema` field of a document to its id.
fn pin_schema_field(properties: &mut Map<String, Value>, schema_id: &str) {
    if let Some(field) = properties.get_mut("schema").and_then(Value::as_object_mut) {
        field.insert("const".into(), json!(schema_id));
    }
}

/// A stream line: the document itself, with its `schema` field pinned.
///
/// Nothing wraps it, so this is the whole thing a reader validates.
fn line_schema(schema_id: &str, mut line: Value) -> Value {
    if let Some(object) = line.as_object_mut() {
        object.insert("title".into(), json!(schema_id));
        if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
            pin_schema_field(properties, schema_id);
        }
    }
    line
}

/// How one schema's machine-readable output is framed.
///
/// Almost every command prints one §7 envelope per invocation and `--json`
/// selects it. `canvas watch` is the documented exception: its stream is its
/// own contract, so `--jsonl` prints one self-describing `event@1` document
/// per line and closes with one `watch@1` envelope, and `--json` is refused
/// with exit 2 rather than answered with something the stream is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    /// One §7 envelope per invocation, printed by `--json`.
    Envelope,
    /// The closing envelope of the `--jsonl` stream.
    StreamSummary,
    /// One self-describing document per line of the `--jsonl` stream.
    StreamLine,
}

/// The form one registered schema is printed in.
fn form_of(schema_id: &str) -> Form {
    match schema_id {
        SCHEMA_EVENT => Form::StreamLine,
        SCHEMA_WATCH => Form::StreamSummary,
        _ => Form::Envelope,
    }
}

/// How a reader gets these bytes: the flag, the framing, and what is refused.
fn output_section(form: Form) -> Value {
    match form {
        Form::Envelope => json!({
            "$comment": "One JSON document per invocation (SPEC §7). A command that aborts prints the error branch (SPEC §14).",
            "form": "envelope",
            "flag": "--json",
        }),
        Form::StreamSummary => json!({
            "$comment": "`canvas watch --jsonl` streams `canvas-cli/event@1` documents, one per line, and closes with this envelope. `--jsonl` is the machine-readable form; `--json` is refused with exit 2, because the stream is its own contract (SPEC §7).",
            "form": "envelope",
            "flag": "--jsonl",
            "printed_by": "canvas watch --jsonl",
            "refuses": ["--json"],
        }),
        Form::StreamLine => json!({
            "$comment": "One self-describing document per line of `canvas watch --jsonl`. No §7 envelope wraps it: the run reports itself in the closing `canvas-cli/watch@1` document instead. `--jsonl` is the machine-readable form; `--json` is refused with exit 2 (SPEC §7).",
            "form": "jsonl",
            "flag": "--jsonl",
            "printed_by": "canvas watch --jsonl",
            "refuses": ["--json"],
        }),
    }
}

/// The `schema@1` document for one registered schema.
///
/// `output` says how the bytes are framed and which flag prints them.
///
/// For an envelope form, `envelope` is what the flag prints: either the
/// command's own result, or the `error@1` document §14 requires when the
/// command aborts. `result` and `error` repeat the two branches on their own,
/// so a caller can validate just the payload it cares about.
///
/// For a stream line there is no envelope and no error branch to describe:
/// the document carries `line`, which is the whole thing a reader validates.
#[must_use]
pub fn document(entry: &SchemaEntry) -> Value {
    let (result, source) = result_schema(entry);
    let form = form_of(entry.id);
    let mut document = Map::new();
    document.insert("$schema".into(), json!(DIALECT));
    document.insert("contract".into(), json!(SCHEMA_SCHEMA));
    document.insert("command".into(), json!(entry_command(entry)));
    document.insert("schema".into(), json!(entry.id));
    document.insert("result_source".into(), json!(source));
    document.insert("output".into(), output_section(form));
    match form {
        Form::StreamLine => {
            document.insert("line".into(), line_schema(entry.id, result));
        }
        Form::Envelope | Form::StreamSummary => {
            let success = envelope_schema(entry.id, result.clone());
            let error = envelope_schema(SCHEMA_ERROR, schema_of::<ErrorResult>());
            document.insert(
                "envelope".into(),
                json!({
                    "$comment": "One JSON document per invocation (SPEC §7). \
                                 A command that aborts prints the error branch (SPEC §14).",
                    // Both branches are objects. The union says so too, because
                    // a validator may require a type before it reads `oneOf`.
                    "type": "object",
                    "oneOf": [success, error],
                }),
            );
            document.insert("result".into(), result);
            document.insert("error".into(), error);
        }
    }
    Value::Object(document)
}

/// The `schema@1` document for a command name, if one is registered.
#[must_use]
pub fn document_for_command(name: &str) -> Option<Value> {
    registry::entry_for_command(name).map(document)
}

/// The `schema@1` document for a schema id and one of its result shapes.
///
/// `canvas mcp` describes a tool's output this way, so a tool and the CLI
/// cannot disagree about the shape of the same result. A schema whose
/// Appendix D row lists several shapes — `receipts@1`, `cache@1`, `config@1`,
/// `identity@1` — has one entry per shape, so the variant selects which one;
/// `None` takes the first, which is the shape the bare command prints.
#[must_use]
pub fn document_for_schema(schema_id: &str, variant: Option<&str>) -> Option<Value> {
    registry::entry_for_schema(schema_id, variant).map(document)
}

/// The full command name of one entry: the command, plus its variant when the
/// schema has more than one result shape.
#[must_use]
pub fn entry_command(entry: &SchemaEntry) -> String {
    let command = command_name(entry.id);
    match entry.variant {
        Some(variant) => format!("{command} {variant}"),
        None => command,
    }
}

/// The command name a schema id belongs to: `canvas-cli/auth_status@1` is
/// printed by `canvas auth status`.
#[must_use]
pub fn command_name(schema_id: &str) -> String {
    schema_id
        .trim_start_matches("canvas-cli/")
        .split('@')
        .next()
        .unwrap_or_default()
        .replace('_', " ")
}

/// `canvas schema --list`: every registered schema, one per line.
#[must_use]
pub fn list() -> String {
    let mut lines: Vec<String> = registry::all_schemas()
        .iter()
        .map(|entry| format!("{}\t{}", entry_command(entry), entry.id))
        .collect();
    lines.sort();
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_schema_has_a_document() {
        for entry in registry::all_schemas() {
            let document = document(entry);
            assert_eq!(document["schema"], entry.id, "{}", entry.id);
            assert_eq!(document["contract"], SCHEMA_SCHEMA);
            assert!(
                document["output"]["flag"].is_string(),
                "{} does not say which flag prints it",
                entry.id
            );
            if form_of(entry.id) == Form::StreamLine {
                // A stream line is the document. There is nothing around it,
                // so there is no envelope and no error branch to describe.
                assert!(
                    document["line"]["properties"]["schema"]["const"] == json!(entry.id),
                    "{} does not pin its own schema id",
                    entry.id
                );
                assert!(document.get("envelope").is_none(), "{}", entry.id);
                assert!(document.get("error").is_none(), "{}", entry.id);
                continue;
            }
            assert!(
                document["envelope"]["oneOf"][0]["properties"]["result"].is_object(),
                "{} has no result schema",
                entry.id
            );
            assert_eq!(
                document["envelope"]["oneOf"][1]["properties"]["schema"]["const"], SCHEMA_ERROR,
                "{} has no error branch",
                entry.id
            );
        }
    }

    /// The event stream is one document per line, and the page says so.
    #[test]
    fn the_event_schema_describes_a_line_and_claims_no_envelope() {
        let entry = registry::entry_for_schema(SCHEMA_EVENT, None).expect("event is registered");
        let document = document(&entry.clone());
        assert_eq!(document["output"]["form"], "jsonl");
        assert_eq!(document["output"]["flag"], "--jsonl");
        assert_eq!(document["output"]["refuses"], json!(["--json"]));
        assert_eq!(document["output"]["printed_by"], "canvas watch --jsonl");
        // The line is the whole document: `schema` is pinned, and the fields
        // an `event@1` line carries are named at the top level, not under a
        // `result` key of an envelope that never exists.
        assert_eq!(
            document["line"]["properties"]["schema"]["const"],
            SCHEMA_EVENT
        );
        for field in [
            "cursor",
            "kind",
            "observed_at",
            "observed_at_local",
            "identity",
            "generation",
            "dataset",
            "scope",
            "entity_key",
            "before",
            "after",
        ] {
            // `before` and `after` carry allowlisted JSON, which schemars
            // describes as the always-true schema, so presence is the test.
            assert!(
                document["line"]["properties"].get(field).is_some(),
                "the line does not describe {field}"
            );
        }
        assert!(document["line"]["properties"]["outcome"].is_null());
        // The result type is the source, not a fixture guess.
        assert_eq!(document["result_source"], "result type");
    }

    /// The closing summary is an envelope, and the page says which flag
    /// prints it and which one is refused.
    #[test]
    fn the_watch_schema_says_json_is_refused() {
        let entry = registry::entry_for_schema(SCHEMA_WATCH, None).expect("watch is registered");
        let document = document(&entry.clone());
        assert_eq!(document["output"]["form"], "envelope");
        assert_eq!(document["output"]["flag"], "--jsonl");
        assert_eq!(document["output"]["refuses"], json!(["--json"]));
        let comment = document["output"]["$comment"].as_str().unwrap_or_default();
        assert!(
            comment.contains("`--json` is refused with exit 2"),
            "{comment}"
        );
        assert!(comment.contains("canvas-cli/event@1"), "{comment}");
        // It is still a §7 envelope, with both branches.
        assert_eq!(
            document["envelope"]["oneOf"][0]["properties"]["schema"]["const"],
            SCHEMA_WATCH
        );
        assert_eq!(
            document["envelope"]["oneOf"][1]["properties"]["schema"]["const"],
            SCHEMA_ERROR
        );
        assert_eq!(document["result_source"], "result type");
    }

    /// The generated envelope must describe the envelope the CLI writes.
    #[test]
    fn the_envelope_schema_names_every_field_of_a_rendered_envelope() {
        use crate::output::now::with_canvas_now;
        with_canvas_now("2026-09-09T17:05:12Z", || {
            let entry = registry::all_schemas()
                .iter()
                .find(|entry| entry.id == SCHEMA_COURSES)
                .expect("courses is registered");
            let document = document(entry);
            let properties = document["envelope"]["oneOf"][0]["properties"]
                .as_object()
                .expect("envelope properties");
            let envelope = Envelope::new(SCHEMA_COURSES, Some("default"), None)
                .with_result(serde_json::json!({ "courses": [] }));
            let rendered = serde_json::to_value(&envelope).expect("serialize");
            for key in rendered.as_object().expect("object").keys() {
                assert!(properties.contains_key(key), "schema omits {key}");
            }
            for key in properties.keys() {
                assert!(
                    rendered.get(key).is_some(),
                    "schema names {key}, which no envelope carries"
                );
            }
        });
    }

    #[test]
    fn command_names_come_from_the_schema_id() {
        assert_eq!(command_name("canvas-cli/todo@1"), "todo");
        assert_eq!(command_name("canvas-cli/auth_status@1"), "auth status");
    }

    #[test]
    fn the_list_covers_the_registry() {
        let list = list();
        assert_eq!(
            list.lines().count(),
            registry::all_schemas().len(),
            "list={list}"
        );
        assert!(list.contains("todo\tcanvas-cli/todo@1"));
        assert!(list.ends_with('\n'));
    }

    #[test]
    fn documents_are_stable() {
        let entry = registry::all_schemas()
            .iter()
            .find(|entry| entry.id == SCHEMA_CALENDAR)
            .expect("calendar is registered");
        insta::assert_json_snapshot!("calendar_schema_document", document(entry));
    }
}
