//! Schema conformance: every `--json` snapshot against its registry fixture.
//!
//! The registry fixture in `src/output/schemas/` is the Appendix D shape for
//! one `result` payload. This module reads every JSON snapshot the suite
//! produced and checks the live payload against the fixture for its `schema`:
//!
//! * **field presence** — Appendix D says every listed field is always present,
//!   so the key sets must match exactly, in both directions;
//! * **nullability** — a fixture value of `null` declares the field nullable;
//!   any other fixture value says the field carries one, with an explicit
//!   allowlist for the Appendix D `T?` fields whose fixture shows an example;
//! * **arrays are never null** — a fixture array is an array in the live result;
//! * **sort order** — the Appendix D `Sort` column, per schema.
//!
//! The envelope rules from §7 are checked on the same snapshots: the fixed key
//! set, decimal-string ids, and a `<name>_local` sibling for every `ts+local`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// One Appendix D payload shape and the file that holds it.
struct Shape {
    schema: &'static str,
    fixture_file: &'static str,
    fixture: &'static str,
}

macro_rules! shapes {
    ($(($schema:expr, $file:literal)),* $(,)?) => {
        &[$(Shape {
            schema: $schema,
            fixture_file: $file,
            fixture: include_str!(concat!("../../src/output/schemas/", $file)),
        }),*]
    };
}

/// Every registry fixture, including the per-variant shapes of a schema whose
/// Appendix D row lists more than one `result`.
const SHAPES: &[Shape] = shapes![
    ("canvas-cli/courses@1", "courses.json"),
    ("canvas-cli/course@1", "course.json"),
    ("canvas-cli/todo@1", "todo.json"),
    ("canvas-cli/assignments@1", "assignments.json"),
    ("canvas-cli/assignment@1", "assignment.json"),
    ("canvas-cli/submit@1", "submit.json"),
    ("canvas-cli/plan@1", "plan.json"),
    ("canvas-cli/submission@1", "submission.json"),
    ("canvas-cli/receipt@1", "receipt.json"),
    ("canvas-cli/receipts@1", "receipts.json"),
    ("canvas-cli/receipts@1", "receipts_show.json"),
    ("canvas-cli/receipts@1", "receipts_export.json"),
    ("canvas-cli/receipts@1", "receipts_acknowledge.json"),
    ("canvas-cli/verify@1", "verify.json"),
    ("canvas-cli/reconcile@1", "reconcile.json"),
    ("canvas-cli/operation@1", "operation.json"),
    (
        "canvas-cli/operation_reconcile@1",
        "operation_reconcile.json"
    ),
    ("canvas-cli/grades@1", "grades.json"),
    ("canvas-cli/files@1", "files.json"),
    ("canvas-cli/modules@1", "modules.json"),
    ("canvas-cli/download@1", "download.json"),
    ("canvas-cli/announcements@1", "announcements.json"),
    ("canvas-cli/announcement@1", "announcement.json"),
    ("canvas-cli/pages@1", "pages.json"),
    ("canvas-cli/page@1", "page.json"),
    ("canvas-cli/syllabus@1", "syllabus.json"),
    ("canvas-cli/discussions@1", "discussions.json"),
    ("canvas-cli/discussion@1", "discussion.json"),
    ("canvas-cli/inbox@1", "inbox.json"),
    ("canvas-cli/conversation@1", "conversation.json"),
    ("canvas-cli/inbox_unread@1", "inbox_unread.json"),
    ("canvas-cli/calendar@1", "calendar.json"),
    ("canvas-cli/open@1", "open.json"),
    ("canvas-cli/sync@1", "sync.json"),
    ("canvas-cli/cache@1", "cache_stats.json"),
    ("canvas-cli/cache@1", "cache_clear.json"),
    ("canvas-cli/cache@1", "cache_path.json"),
    ("canvas-cli/alias@1", "alias.json"),
    ("canvas-cli/auth_status@1", "auth_status.json"),
    ("canvas-cli/auth_login@1", "auth_login.json"),
    ("canvas-cli/auth_logout@1", "auth_logout.json"),
    ("canvas-cli/identity@1", "identity.json"),
    ("canvas-cli/identity@1", "identity_remove.json"),
    ("canvas-cli/config@1", "config.json"),
    ("canvas-cli/config@1", "config_set.json"),
    ("canvas-cli/config@1", "config_path.json"),
    ("canvas-cli/doctor@1", "doctor.json"),
    ("canvas-cli/version@1", "version.json"),
    ("canvas-cli/error@1", "error.json"),
    ("canvas-cli/watch@1", "watch.json"),
    ("canvas-cli/event@1", "event.json"),
];

/// Appendix D `T?` fields whose fixture shows an example value instead of null.
///
/// A fixture can only declare a field nullable by holding `null` there, and a
/// fixture that shows every optional field as null would stop describing the
/// payload. So each entry below was read back against the Appendix D row for
/// its schema and carries a `?` there; a live `null` under any other non-null
/// fixture field is a conformance failure. Paths are `<schema>:<dotted path>`,
/// with `[]` for an array step.
const NULLABLE_WITH_EXAMPLE: &[&str] = &[
    "canvas-cli/alias@1:aliases[].course_code",
    "canvas-cli/announcement@1:announcement.course_code",
    "canvas-cli/announcements@1:announcements[].author",
    "canvas-cli/assignment@1:assignment.status.excused",
    "canvas-cli/assignment@1:assignment.status.late",
    "canvas-cli/assignments@1:assignments[].status.excused",
    "canvas-cli/assignments@1:assignments[].status.late",
    "canvas-cli/auth_logout@1:backend",
    "canvas-cli/calendar@1:items[].due_at",
    "canvas-cli/calendar@1:items[].due_at_local",
    "canvas-cli/calendar@1:items[].html_url",
    "canvas-cli/course@1:course.modules_count",
    "canvas-cli/course@1:course.time_zone",
    "canvas-cli/download@1:courses[].files[].verify",
    "canvas-cli/error@1:http_status",
    "canvas-cli/files@1:files[].folder_path",
    "canvas-cli/files@1:files[].updated_at",
    "canvas-cli/grades@1:course",
    "canvas-cli/grades@1:courses[].grades.period.id",
    "canvas-cli/grades@1:courses[].grades.period.title",
    "canvas-cli/plan@1:plan.approval",
    "canvas-cli/plan@1:plan.assignment_name",
    "canvas-cli/plan@1:plan.comment_chars",
    "canvas-cli/plan@1:plan.course_code",
    "canvas-cli/plan@1:plan.due_at",
    "canvas-cli/receipt@1:approval",
    "canvas-cli/receipt@1:plan_id",
    "canvas-cli/receipts@1:journal.approval",
    "canvas-cli/receipts@1:journal.plan_id",
    "canvas-cli/receipts@1:journal.course_code",
    "canvas-cli/receipts@1:journal.posted.excused",
    "canvas-cli/receipts@1:journals[].course_code",
    "canvas-cli/receipts@1:journals[].posted.excused",
    "canvas-cli/receipts@1:journals[].approval",
    "canvas-cli/receipts@1:journals[].plan_id",
    "canvas-cli/receipts@1:receipt.approval",
    "canvas-cli/receipts@1:receipt.course_code",
    "canvas-cli/receipts@1:receipt.plan_id",
    "canvas-cli/receipts@1:receipt.posted.excused",
    "canvas-cli/receipts@1:receipt.readback.late",
    "canvas-cli/receipts@1:receipt.readback.submitted_at",
    "canvas-cli/receipts@1:receipt.readback.submitted_at_local",
    "canvas-cli/reconcile@1:attribution",
    "canvas-cli/reconcile@1:posted",
    "canvas-cli/reconcile@1:receipt_id",
    "canvas-cli/submission@1:history[].submitted_at",
    "canvas-cli/submission@1:history[].submitted_at_local",
    "canvas-cli/submission@1:submission.excused",
    "canvas-cli/submission@1:submission.posted_at",
    "canvas-cli/submit@1:attribution",
    "canvas-cli/submit@1:posted",
    "canvas-cli/submit@1:posted.excused",
    "canvas-cli/submit@1:receipt_id",
    "canvas-cli/watch@1:cursor",
    "canvas-cli/watch@1:since",
    "canvas-cli/todo@1:items[].scheduled_at",
    "canvas-cli/todo@1:items[].scheduled_at_local",
    "canvas-cli/todo@1:items[].status.graded",
    "canvas-cli/todo@1:items[].status.submitted",
];

/// Fields whose Appendix D type is whatever the underlying value is.
///
/// `config get|set` report a config value, which is a string, a number or a
/// bool depending on the key, so the fixture's example fixes no type.
const ANY_TYPE: &[&str] = &["canvas-cli/config@1:value", "canvas-cli/config@1:previous"];

/// Objects Appendix D declares as free-form, whose keys the fixture cannot fix.
const OPAQUE_OBJECTS: &[&str] = &["canvas-cli/error@1:details"];

/// Appendix D `Sort` column, as the key path each array is ordered by.
///
/// `!` marks a descending key; `?` marks a key whose absent values sort last.
const SORTS: &[(&str, &[&str])] = &[
    ("canvas-cli/courses@1:courses", &["code", "id"]),
    (
        "canvas-cli/todo@1:items",
        &["?scheduled_at|due_at", "course_code", "id"],
    ),
    ("canvas-cli/assignments@1:assignments", &["?due_at", "id"]),
    ("canvas-cli/submit@1:candidates", &["attempt"]),
    ("canvas-cli/submission@1:history", &["attempt"]),
    (
        "canvas-cli/receipts@1:journals",
        &["!created_at", "journal_id"],
    ),
    ("canvas-cli/verify@1:files", &["canvas_file_id"]),
    ("canvas-cli/reconcile@1:candidates", &["attempt"]),
    ("canvas-cli/grades@1:courses", &["course.code"]),
    ("canvas-cli/files@1:files", &["folder_path", "name", "id"]),
    ("canvas-cli/modules@1:modules", &["position"]),
    ("canvas-cli/sync@1:datasets", &["dataset", "scope"]),
    ("canvas-cli/watch@1:datasets", &["dataset", "scope"]),
    ("canvas-cli/alias@1:aliases", &["name"]),
    ("canvas-cli/identity@1:identities", &["key"]),
];

/// Envelope keys SPEC §7 fixes for every invocation.
const ENVELOPE_KEYS: [&str; 11] = [
    "schema",
    "generated_at",
    "profile",
    "identity",
    "freshness",
    "requests",
    "partial",
    "warnings",
    "outcome",
    "exit",
    "result",
];

#[test]
fn the_shape_table_covers_every_registry_fixture() {
    let dir = repo_path("crates/canvas-cli/src/output/schemas");
    let on_disk: BTreeSet<String> = std::fs::read_dir(&dir)
        .expect("the schema directory")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| Path::new(name).extension().is_some_and(|ext| ext == "json"))
        .collect();
    let listed: BTreeSet<String> = SHAPES
        .iter()
        .map(|shape| shape.fixture_file.to_owned())
        .collect();
    assert_eq!(
        on_disk,
        listed,
        "every fixture in {} belongs to exactly one Appendix D shape",
        dir.display()
    );
}

#[test]
fn every_json_snapshot_matches_its_registry_fixture() {
    let snapshots = json_snapshots();
    assert!(
        snapshots.len() > 20,
        "the suite produced {} JSON snapshots; the run is incomplete",
        snapshots.len()
    );
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (name, envelope) in &snapshots {
        check_envelope(name, envelope);
        let schema = envelope["schema"].as_str().expect("schema is a string");
        let result = &envelope["result"];
        let shape = pick_shape(name, schema, result);
        seen.insert(shape.fixture_file);
        let fixture: Value =
            serde_json::from_str(shape.fixture).expect("the fixture parses as JSON");
        compare(name, schema, "", &fixture, result);
        check_sorts(name, schema, result);
    }
    assert!(
        seen.contains("courses.json") && seen.contains("error.json"),
        "the suite reached both a data schema and the error schema"
    );
}

/// Every envelope carries the §7 key set and nothing else.
fn check_envelope(name: &str, envelope: &Value) {
    let keys: BTreeSet<&str> = envelope
        .as_object()
        .expect("an envelope is an object")
        .keys()
        .map(String::as_str)
        .collect();
    let expected: BTreeSet<&str> = ENVELOPE_KEYS.into_iter().collect();
    assert_eq!(keys, expected, "{name}: envelope keys");
    for key in ["freshness", "partial", "warnings"] {
        assert!(envelope[key].is_array(), "{name}: {key} is never null");
    }
    check_ids_and_local_siblings(name, &envelope["result"]);
}

/// Ids are decimal strings, and no `_local` field stands without its base (§7).
///
/// The other half of the `ts+local` rule — every field Appendix D marks
/// `ts+local` really carries a `_local` sibling — is enforced by [`compare`],
/// which requires the live key set to equal the fixture's in both directions.
fn check_ids_and_local_siblings(name: &str, value: &Value) {
    match value {
        Value::Object(map) => {
            for (key, entry) in map {
                if key == "id" || key.ends_with("_id") {
                    if let Some(text) = entry.as_str() {
                        assert!(
                            text.bytes().all(|b| b.is_ascii_digit())
                                || !text.bytes().all(|b| b.is_ascii_alphanumeric()),
                            "{name}: {key} = {text:?} is neither a decimal id nor a uuid"
                        );
                    }
                    assert!(
                        !entry.is_number(),
                        "{name}: {key} is a number; §7 makes ids strings"
                    );
                }
                check_ids_and_local_siblings(name, entry);
            }
            for key in map.keys() {
                if let Some(base) = key.strip_suffix("_local") {
                    assert!(
                        map.contains_key(base),
                        "{name}: {key} has no {base} sibling"
                    );
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                check_ids_and_local_siblings(name, item);
            }
        }
        _ => {}
    }
}

/// The shape whose top-level keys match this payload.
fn pick_shape(name: &str, schema: &str, result: &Value) -> &'static Shape {
    let keys = |value: &Value| -> BTreeSet<String> {
        value
            .as_object()
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default()
    };
    let live = keys(result);
    let candidates: Vec<&Shape> = SHAPES.iter().filter(|s| s.schema == schema).collect();
    assert!(
        !candidates.is_empty(),
        "{name}: no registry fixture for schema {schema}"
    );
    for candidate in &candidates {
        let fixture: Value = serde_json::from_str(candidate.fixture).expect("fixture JSON");
        if keys(&fixture) == live {
            return candidate;
        }
    }
    panic!(
        "{name}: no {schema} fixture has the keys {live:?}; candidates are {:?}",
        candidates
            .iter()
            .map(|c| c.fixture_file)
            .collect::<Vec<_>>()
    );
}

/// Compare one live value with the fixture that declares its shape.
fn compare(name: &str, schema: &str, path: &str, fixture: &Value, live: &Value) {
    if OPAQUE_OBJECTS.contains(&format!("{schema}:{path}").as_str()) {
        assert!(live.is_object(), "{name}: {path} is {live}, not an object");
        return;
    }
    match fixture {
        Value::Object(shape) => {
            let Some(actual) = live.as_object() else {
                assert!(
                    live.is_null() && is_nullable(schema, path),
                    "{name}: {path} should be an object, found {live}"
                );
                return;
            };
            let expected: BTreeSet<&String> = shape.keys().collect();
            let found: BTreeSet<&String> = actual.keys().collect();
            assert_eq!(
                expected, found,
                "{name}: {path} field set differs from the fixture"
            );
            for (key, entry) in shape {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                compare(name, schema, &child, entry, &actual[key]);
            }
        }
        Value::Array(shape) => {
            let actual = live.as_array().unwrap_or_else(|| {
                panic!("{name}: {path} is {live}; Appendix D says arrays are never null")
            });
            let Some(element) = shape.first() else {
                return;
            };
            let child = format!("{path}[]");
            for item in actual {
                compare(name, schema, &child, element, item);
            }
        }
        Value::Null => {}
        _ => {
            if live.is_null() {
                assert!(
                    is_nullable(schema, path),
                    "{name}: {path} is null, but the fixture declares {fixture}"
                );
                return;
            }
            if !ANY_TYPE.contains(&format!("{schema}:{path}").as_str()) {
                assert_eq!(
                    kind(fixture),
                    kind(live),
                    "{name}: {path} is {live}, the fixture declares {fixture}"
                );
            }
        }
    }
}

fn is_nullable(schema: &str, path: &str) -> bool {
    NULLABLE_WITH_EXAMPLE.contains(&format!("{schema}:{path}").as_str())
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Each array named in the Appendix D `Sort` column is in that order.
fn check_sorts(name: &str, schema: &str, result: &Value) {
    for (target, keys) in SORTS {
        let Some((sort_schema, field)) = target.split_once(':') else {
            continue;
        };
        if sort_schema != schema {
            continue;
        }
        let Some(items) = result.get(field).and_then(Value::as_array) else {
            continue;
        };
        let ordered: Vec<Vec<SortKey>> = items.iter().map(|item| sort_key(item, keys)).collect();
        for pair in ordered.windows(2) {
            assert!(
                pair[0] <= pair[1],
                "{name}: {schema}.{field} is not sorted by {keys:?}"
            );
        }
    }
}

/// A comparable key: `(absent, descending-flip, value)` per Appendix D.
type SortKey = (bool, bool, String);

fn sort_key(item: &Value, keys: &[&str]) -> Vec<SortKey> {
    keys.iter()
        .map(|key| {
            let descending = key.starts_with('!');
            let key = key.trim_start_matches(['!', '?']);
            let value = key
                .split('|')
                .filter_map(|alternative| lookup(item, alternative))
                .find(|value| !value.is_null());
            let text = value.map_or_else(String::new, render_sortable);
            let absent = text.is_empty();
            let text = if descending { flip(&text) } else { text };
            (absent, descending, text)
        })
        .collect()
}

fn lookup<'a>(item: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(item, |value, step| value.get(step))
}

/// Render a scalar so lexical order matches Appendix D's order.
fn render_sortable(value: &Value) -> String {
    match value {
        Value::Number(n) => format!("{:020}", n.as_i64().unwrap_or_default()),
        Value::String(s) if s.bytes().all(|b| b.is_ascii_digit()) => format!("{s:0>20}"),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Invert a key so a descending column compares like an ascending one.
fn flip(text: &str) -> String {
    text.bytes().map(|b| char::from(u8::MAX - b)).collect()
}

/// Every JSON snapshot this suite wrote, as `(name, envelope)`.
fn json_snapshots() -> BTreeMap<String, Value> {
    let dir = repo_path("crates/canvas-cli/tests/e2e/snapshots");
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).expect("the snapshot directory") {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "snap") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        // `---` header, metadata, `---`, then the snapshot body.
        let Some(body) = text.splitn(3, "---\n").nth(2) else {
            continue;
        };
        let body = body.trim_start();
        if !body.starts_with('{') {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(body) else {
            continue;
        };
        if value.get("schema").is_some() && value.get("result").is_some() {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            out.insert(name, value);
        }
    }
    out
}

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// A journal created before plans exposes `plan_id` and `approval` as null.
///
/// Appendix D's nullable convention makes that the correct legacy shape, and
/// REPORT §3.5 makes `plan@1`'s `approval` null until a person approves. Every
/// submission in this suite runs through the plan path, so no snapshot carries
/// the legacy shape yet and nothing else would notice a fixture that declares
/// these fields as always-present.
#[test]
fn the_plan_fields_are_nullable_in_every_shape_that_carries_them() {
    fn blank(value: &mut Value, keys: &[&str]) {
        match value {
            Value::Object(map) => {
                for (key, entry) in map.iter_mut() {
                    if keys.contains(&key.as_str()) {
                        *entry = Value::Null;
                    } else {
                        blank(entry, keys);
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|item| blank(item, keys)),
            _ => {}
        }
    }

    for shape in SHAPES {
        // A plan names itself, so only its approval can be null. So does an
        // operation journal: it exists only because a plan admitted it, and
        // the unique index on `plan_id` is what makes that one journal.
        let names_its_own_plan =
            matches!(shape.schema, "canvas-cli/plan@1" | "canvas-cli/operation@1");
        let keys: &[&str] = if names_its_own_plan {
            &["approval"]
        } else {
            &["plan_id", "approval"]
        };
        let fixture: Value = serde_json::from_str(shape.fixture).expect("fixture JSON");
        let mut legacy = fixture.clone();
        blank(&mut legacy, keys);
        if legacy == fixture {
            continue;
        }
        compare(shape.fixture_file, shape.schema, "", &fixture, &legacy);
    }
}
