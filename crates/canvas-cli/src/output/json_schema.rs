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
    ConversationResult, CourseResult, CoursesResult, DiscussionResult, DiscussionsResult,
    DownloadResult, FilesResult, GradesResult, InboxResult, InboxUnreadResult, ModulesResult,
    OperationReconcileResult, OperationResult, PageResult, PagesResult, PlanResult, SCHEMA_ALIAS,
    SCHEMA_ANNOUNCEMENT, SCHEMA_ANNOUNCEMENTS, SCHEMA_CACHE, SCHEMA_CALENDAR, SCHEMA_CONVERSATION,
    SCHEMA_COURSE, SCHEMA_COURSES, SCHEMA_DISCUSSION, SCHEMA_DISCUSSIONS, SCHEMA_DOWNLOAD,
    SCHEMA_ERROR, SCHEMA_FILES, SCHEMA_GRADES, SCHEMA_INBOX, SCHEMA_INBOX_UNREAD, SCHEMA_MODULES,
    SCHEMA_OPERATION, SCHEMA_OPERATION_RECONCILE, SCHEMA_PAGE, SCHEMA_PAGES, SCHEMA_PLAN,
    SCHEMA_SUBMIT, SCHEMA_SYLLABUS, SCHEMA_SYNC, SchemaEntry, SubmitResult, SyllabusResult,
    SyncResult,
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
    // The variant is part of the key: `cache@1` covers three different result
    // shapes and only `stats` is `CacheStatsResult`, so matching on the id
    // alone advertised the stats shape for `cache clear` and `cache path`.
    let typed = match (entry.id, entry.variant) {
        (SCHEMA_COURSES, _) => Some(schema_of::<CoursesResult>()),
        (SCHEMA_COURSE, _) => Some(schema_of::<CourseResult>()),
        (SCHEMA_ALIAS, _) => Some(schema_of::<AliasResult>()),
        (SCHEMA_SYNC, _) => Some(schema_of::<SyncResult>()),
        (SCHEMA_CACHE, Some("stats")) => Some(schema_of::<CacheStatsResult>()),
        (SCHEMA_SUBMIT, _) => Some(schema_of::<SubmitResult>()),
        (SCHEMA_PLAN, _) => Some(schema_of::<PlanResult>()),
        (SCHEMA_OPERATION, _) => Some(schema_of::<OperationResult>()),
        (SCHEMA_OPERATION_RECONCILE, _) => Some(schema_of::<OperationReconcileResult>()),
        (SCHEMA_FILES, _) => Some(schema_of::<FilesResult>()),
        (SCHEMA_MODULES, _) => Some(schema_of::<ModulesResult>()),
        (SCHEMA_GRADES, _) => Some(schema_of::<GradesResult>()),
        (SCHEMA_DOWNLOAD, _) => Some(schema_of::<DownloadResult>()),
        (SCHEMA_ANNOUNCEMENTS, _) => Some(schema_of::<AnnouncementsResult>()),
        (SCHEMA_ANNOUNCEMENT, _) => Some(schema_of::<AnnouncementResult>()),
        (SCHEMA_CALENDAR, _) => Some(schema_of::<CalendarResult>()),
        (SCHEMA_PAGES, _) => Some(schema_of::<PagesResult>()),
        (SCHEMA_PAGE, _) => Some(schema_of::<PageResult>()),
        (SCHEMA_SYLLABUS, _) => Some(schema_of::<SyllabusResult>()),
        (SCHEMA_DISCUSSIONS, _) => Some(schema_of::<DiscussionsResult>()),
        (SCHEMA_DISCUSSION, _) => Some(schema_of::<DiscussionResult>()),
        (SCHEMA_INBOX, _) => Some(schema_of::<InboxResult>()),
        (SCHEMA_CONVERSATION, _) => Some(schema_of::<ConversationResult>()),
        (SCHEMA_INBOX_UNREAD, _) => Some(schema_of::<InboxUnreadResult>()),
        (SCHEMA_ERROR, _) => Some(schema_of::<ErrorResult>()),
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

/// The name of one entry: the command that prints it, as a person types it.
///
/// The entry carries the command, because it cannot be derived from the schema
/// id — `conversation@1` is printed by `inbox show`. A document no command
/// prints falls back to the schema's own short name, so `canvas schema error`
/// still resolves.
#[must_use]
pub fn entry_command(entry: &SchemaEntry) -> String {
    match entry.command {
        Some(command) => command.to_owned(),
        None => command_name(entry.id),
    }
}

/// Whether this entry describes a command's output or a standalone document.
#[must_use]
pub fn entry_kind(entry: &SchemaEntry) -> &'static str {
    if entry.command.is_some() {
        "command"
    } else {
        "document"
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
///
/// Three tab-separated columns: the name, the schema id, and whether the name
/// is a command a person can run or a document no command prints. Every name
/// in the first column resolves with `canvas schema <name>`.
#[must_use]
pub fn list() -> String {
    let mut lines: Vec<String> = registry::all_schemas()
        .iter()
        .map(|entry| {
            format!(
                "{}\t{}\t{}",
                entry_command(entry),
                entry.id,
                entry_kind(entry)
            )
        })
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
        assert!(list.contains("todo\tcanvas-cli/todo@1\tcommand"));
        assert!(list.ends_with('\n'));
    }

    /// Every name the listing prints must resolve, and every command it calls
    /// a command must be one the binary actually has.
    ///
    /// Deriving the name from the schema id listed `conversation` and
    /// `inbox unread` as commands and left `inbox show` and
    /// `inbox unread-count` unreachable.
    #[test]
    fn every_listed_name_resolves_to_its_own_entry() {
        for entry in registry::all_schemas() {
            let name = entry_command(entry);
            let found = registry::entry_for_command(&name)
                .unwrap_or_else(|| panic!("{name} does not resolve"));
            assert_eq!(found.id, entry.id, "{name} resolves to another schema");
            assert_eq!(
                found.variant, entry.variant,
                "{name} resolves to another shape"
            );
        }
    }

    #[test]
    fn the_commands_the_listing_names_are_the_commands_the_binary_has() {
        use clap::CommandFactory;
        let cli = crate::cli::Cli::command();
        let path_exists = |path: &str| {
            let mut node = &cli;
            for part in path.split(' ') {
                match node.get_subcommands().find(|sub| sub.get_name() == part) {
                    Some(sub) => node = sub,
                    None => return false,
                }
            }
            true
        };
        for entry in registry::all_schemas() {
            if let Some(command) = entry.command {
                assert!(path_exists(command), "`canvas {command}` is not a command");
            }
        }
    }

    /// Every registry fixture must satisfy the document that describes it.
    ///
    /// A fixture is the example Appendix D publishes and the shape the MCP
    /// `outputSchema` advertises, so a fixture the document rejects means a
    /// host validating a legitimate answer would reject it too. This is the
    /// defect `docs/reviews/code-M8-a2.md` found on the eight M8-a schemas:
    /// their documents were inferred from the fixture and declared every
    /// nullable field non-nullable.
    #[test]
    fn every_fixture_satisfies_its_own_schema() {
        for entry in registry::all_schemas() {
            let value: Value = serde_json::from_str(entry.fixture)
                .unwrap_or_else(|e| panic!("{} fixture is not JSON: {e}", entry.id));
            let (schema, source) = result_schema(entry);
            let mut failures = Vec::new();
            check(&schema, &value, "result", &mut failures);
            assert!(
                failures.is_empty(),
                "{} ({source}) rejects its own fixture:\n  {}",
                entry.id,
                failures.join("\n  ")
            );
        }
    }

    /// The same check, run against a value the schema must refuse, so the
    /// checker above cannot pass by accepting everything.
    #[test]
    fn the_checker_refuses_a_value_the_schema_forbids() {
        let entry = registry::all_schemas()
            .iter()
            .find(|entry| entry.id == SCHEMA_COURSES)
            .expect("courses is registered");
        let (schema, _) = result_schema(entry);
        let mut failures = Vec::new();
        check(
            &schema,
            &json!({ "courses": "not an array" }),
            "result",
            &mut failures,
        );
        assert!(!failures.is_empty(), "a string passed for an array");

        let mut failures = Vec::new();
        check(&schema, &json!({}), "result", &mut failures);
        assert!(!failures.is_empty(), "a missing required property passed");
    }

    /// Validate `value` against the subset of JSON Schema `schema_of` emits.
    ///
    /// The generator inlines every subschema and uses only `type` (one name or
    /// a union), `properties`, `required`, `items`, `enum`, `const`, and the
    /// `oneOf`/`anyOf` unions an enum produces. A dependency that implements
    /// the whole dialect would buy nothing here and would need an Appendix A
    /// row; this covers exactly what the generator writes.
    fn check(schema: &Value, value: &Value, path: &str, failures: &mut Vec<String>) {
        let Some(object) = schema.as_object() else {
            // `true` admits anything; `false` admits nothing.
            if schema.as_bool() == Some(false) {
                failures.push(format!("{path}: schema admits nothing"));
            }
            return;
        };
        if let Some(constant) = object.get("const")
            && constant != value
        {
            failures.push(format!("{path}: {value} is not {constant}"));
            return;
        }
        if let Some(allowed) = object.get("enum").and_then(Value::as_array)
            && !allowed.contains(value)
        {
            failures.push(format!("{path}: {value} is not one of {allowed:?}"));
            return;
        }
        for key in ["oneOf", "anyOf"] {
            if let Some(branches) = object.get(key).and_then(Value::as_array) {
                let ok = branches.iter().any(|branch| {
                    let mut ignored = Vec::new();
                    check(branch, value, path, &mut ignored);
                    ignored.is_empty()
                });
                if !ok {
                    failures.push(format!("{path}: {value} matches no {key} branch"));
                }
                return;
            }
        }
        if let Some(declared) = object.get("type") {
            let names: Vec<&str> = match declared {
                Value::String(name) => vec![name.as_str()],
                Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
                _ => Vec::new(),
            };
            if !names.is_empty() && !names.iter().any(|name| matches_type(name, value)) {
                failures.push(format!("{path}: {} is not {names:?}", kind_of(value)));
                return;
            }
        }
        // `properties` and `required` say nothing about a value that is not an
        // object. A nullable object is `["object", "null"]` with the required
        // list of its object branch, and `null` satisfies it.
        if let Some(map) = value.as_object()
            && let Some(properties) = object.get("properties").and_then(Value::as_object)
        {
            for name in object
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !map.contains_key(name) {
                    failures.push(format!("{path}.{name} is required and absent"));
                }
            }
            for (name, item) in map {
                if let Some(property) = properties.get(name) {
                    check(property, item, &format!("{path}.{name}"), failures);
                }
            }
        }
        if let Some(items) = object.get("items")
            && let Some(array) = value.as_array()
        {
            for (index, item) in array.iter().enumerate() {
                check(items, item, &format!("{path}[{index}]"), failures);
            }
        }
    }

    fn matches_type(name: &str, value: &Value) -> bool {
        match name {
            "null" => value.is_null(),
            "boolean" => value.is_boolean(),
            "string" => value.is_string(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            "number" => value.is_number(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            _ => true,
        }
    }

    fn kind_of(value: &Value) -> &'static str {
        match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
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
