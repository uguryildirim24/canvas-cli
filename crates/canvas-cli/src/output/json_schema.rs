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
    CourseResult, CoursesResult, DownloadResult, FilesResult, GradesResult, ModulesResult,
    PlanResult, SCHEMA_ALIAS, SCHEMA_ANNOUNCEMENT, SCHEMA_ANNOUNCEMENTS, SCHEMA_CACHE,
    SCHEMA_CALENDAR, SCHEMA_COURSE, SCHEMA_COURSES, SCHEMA_DOWNLOAD, SCHEMA_ERROR, SCHEMA_FILES,
    SCHEMA_GRADES, SCHEMA_MODULES, SCHEMA_PLAN, SCHEMA_SUBMIT, SCHEMA_SYNC, SchemaEntry,
    SubmitResult, SyncResult,
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
        SCHEMA_FILES => Some(schema_of::<FilesResult>()),
        SCHEMA_MODULES => Some(schema_of::<ModulesResult>()),
        SCHEMA_GRADES => Some(schema_of::<GradesResult>()),
        SCHEMA_DOWNLOAD => Some(schema_of::<DownloadResult>()),
        SCHEMA_ANNOUNCEMENTS => Some(schema_of::<AnnouncementsResult>()),
        SCHEMA_ANNOUNCEMENT => Some(schema_of::<AnnouncementResult>()),
        SCHEMA_CALENDAR => Some(schema_of::<CalendarResult>()),
        SCHEMA_ERROR => Some(schema_of::<ErrorResult>()),
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
            if let Some(schema_field) = properties.get_mut("schema").and_then(Value::as_object_mut)
            {
                schema_field.insert("const".into(), json!(schema_id));
            }
        }
    }
    envelope
}

/// The `schema@1` document for one registered schema.
///
/// `envelope` is what `--json` prints: either the command's own result, or the
/// `error@1` document §14 requires when the command aborts. `result` and
/// `error` repeat the two branches on their own, so a caller can validate just
/// the payload it cares about.
#[must_use]
pub fn document(entry: &SchemaEntry) -> Value {
    let (result, source) = result_schema(entry);
    let success = envelope_schema(entry.id, result.clone());
    let error = envelope_schema(SCHEMA_ERROR, schema_of::<ErrorResult>());
    let mut document = Map::new();
    document.insert("$schema".into(), json!(DIALECT));
    document.insert("contract".into(), json!(SCHEMA_SCHEMA));
    document.insert("command".into(), json!(entry_command(entry)));
    document.insert("schema".into(), json!(entry.id));
    document.insert("result_source".into(), json!(source));
    document.insert(
        "envelope".into(),
        json!({
            "$comment": "One JSON document per invocation (SPEC §7). \
                         A command that aborts prints the error branch (SPEC §14).",
            // Both branches are objects. The union says so too, because a
            // validator may require a type before it reads `oneOf`.
            "type": "object",
            "oneOf": [success, error],
        }),
    );
    document.insert("result".into(), result);
    document.insert("error".into(), error);
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
