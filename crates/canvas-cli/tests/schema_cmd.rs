//! `canvas schema` (M6-b): the `--json` contract, printed raw.

use assert_cmd::Command;

fn canvas() -> Command {
    Command::cargo_bin("canvas").expect("binary")
}

#[test]
fn the_list_names_every_registered_schema() {
    let assert = canvas().args(["schema", "--list"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let mut seen = std::collections::BTreeSet::new();
    for line in stdout.lines() {
        let mut columns = line.split('\t');
        let (Some(name), Some(id), Some(kind), None) = (
            columns.next(),
            columns.next(),
            columns.next(),
            columns.next(),
        ) else {
            panic!("{line} is not three tab-separated columns");
        };
        assert!(id.starts_with("canvas-cli/"), "{line}");
        assert!(id.ends_with("@1"), "{line}");
        assert!(
            kind == "command" || kind == "document",
            "{line} has an unknown kind"
        );
        // Every row must name a different thing, or a caller cannot ask for
        // the shape it wants.
        assert!(seen.insert(name.to_owned()), "{name} is listed twice");
        // And every name the listing prints must answer for itself.
        canvas().args(["schema", name]).assert().success();
    }
    for expected in [
        "todo\tcanvas-cli/todo@1\tcommand",
        "calendar\tcanvas-cli/calendar@1\tcommand",
        "auth status\tcanvas-cli/auth_status@1\tcommand",
        "receipts list\tcanvas-cli/receipts@1\tcommand",
        "receipts show\tcanvas-cli/receipts@1\tcommand",
        "receipts acknowledge\tcanvas-cli/receipts@1\tcommand",
        // A command name cannot be derived from a schema id: these four were
        // listed as `conversation`, `inbox unread`, `reconcile`, and `verify`.
        "inbox show\tcanvas-cli/conversation@1\tcommand",
        "inbox unread-count\tcanvas-cli/inbox_unread@1\tcommand",
        "submission reconcile\tcanvas-cli/reconcile@1\tcommand",
        "submission verify\tcanvas-cli/verify@1\tcommand",
        // A document is not a command, and the listing says which it is.
        "error\tcanvas-cli/error@1\tdocument",
        "plan\tcanvas-cli/plan@1\tdocument",
        "receipt\tcanvas-cli/receipt@1\tdocument",
    ] {
        assert!(stdout.contains(expected), "missing {expected}: {stdout}");
    }
    // Nothing is listed under a name that is not a command anyone can run.
    for phantom in [
        "\nconversation\t",
        "\ninbox unread\t",
        "\nreconcile\t",
        "\nverify\t",
    ] {
        assert!(
            !format!("\n{stdout}").contains(phantom),
            "{phantom:?} is still listed: {stdout}"
        );
    }
}

/// The schema id keeps naming its own document, so a caller that learned the
/// old name still gets an answer.
#[test]
fn a_schema_id_still_resolves_to_its_entry() {
    for (alias, schema) in [
        ("conversation", "canvas-cli/conversation@1"),
        ("inbox_unread", "canvas-cli/inbox_unread@1"),
        ("reconcile", "canvas-cli/reconcile@1"),
        ("verify", "canvas-cli/verify@1"),
    ] {
        let assert = canvas().args(["schema", alias]).assert().success();
        let document: serde_json::Value =
            serde_json::from_slice(&assert.get_output().stdout).expect("one JSON document");
        assert_eq!(document["schema"], schema, "{alias}");
    }
}

/// A schema with several result shapes answers for each one, and the bare
/// command answers with the shape it prints by itself.
#[test]
fn a_schema_with_several_shapes_describes_each_one() {
    let shape = |command: &[&str]| -> serde_json::Value {
        let mut args = vec!["schema"];
        args.extend_from_slice(command);
        let assert = canvas().args(&args).assert().success();
        serde_json::from_slice(&assert.get_output().stdout).expect("one JSON document")
    };
    let required = |document: &serde_json::Value| -> Vec<String> {
        document["result"]["required"]
            .as_array()
            .expect("required")
            .iter()
            .map(|value| value.as_str().unwrap_or_default().to_owned())
            .collect()
    };
    assert_eq!(required(&shape(&["receipts show"])), ["journal", "receipt"]);
    assert_eq!(
        required(&shape(&["receipts acknowledge"])),
        ["acknowledged_at", "journal_id"]
    );
    // The bare command is the listing, which is what `canvas receipts` prints.
    assert_eq!(required(&shape(&["receipts"])), ["journals"]);
    assert_eq!(shape(&["receipts"])["command"], "receipts list");
}

#[test]
fn a_command_schema_describes_the_envelope_and_the_error_branch() {
    for command in ["todo", "grades", "calendar", "auth_status", "auth-status"] {
        let assert = canvas().args(["schema", command]).assert().success();
        let document: serde_json::Value =
            serde_json::from_slice(&assert.get_output().stdout).expect("one JSON document");
        assert_eq!(document["contract"], "canvas-cli/schema@1");
        assert_eq!(
            document["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        let branches = document["envelope"]["oneOf"]
            .as_array()
            .expect("two envelope branches");
        assert_eq!(branches.len(), 2);
        assert_eq!(
            branches[1]["properties"]["schema"]["const"], "canvas-cli/error@1",
            "{command} has no domain-error branch"
        );
        let required = branches[0]["required"]
            .as_array()
            .expect("required envelope fields");
        for field in [
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
        ] {
            assert!(
                required.iter().any(|value| value == field),
                "{command} does not require {field}"
            );
        }
        assert!(document["result"].is_object(), "{command} has no result");
    }
}

/// The stream is its own contract (SPEC §7), and the page says so: an
/// `event@1` line claims no envelope, and the `watch@1` page names the flag
/// that prints it and the one that is refused.
#[test]
fn the_stream_schemas_describe_the_jsonl_contract() {
    let document = |command: &str| -> serde_json::Value {
        let assert = canvas().args(["schema", command]).assert().success();
        serde_json::from_slice(&assert.get_output().stdout).expect("one JSON document")
    };

    let event = document("event");
    assert_eq!(event["schema"], "canvas-cli/event@1");
    assert_eq!(event["output"]["form"], "jsonl");
    assert_eq!(event["output"]["flag"], "--jsonl");
    assert_eq!(event["output"]["refuses"][0], "--json");
    // No envelope is claimed, and no error branch: a line is never wrapped,
    // and a failed run reports itself in the closing `watch@1` document.
    assert!(event["envelope"].is_null(), "{event}");
    assert!(event["error"].is_null(), "{event}");
    assert_eq!(
        event["line"]["properties"]["schema"]["const"],
        "canvas-cli/event@1"
    );
    assert!(
        event["line"]["properties"]["cursor"].is_object(),
        "the line does not describe its cursor: {event}"
    );

    let watch = document("watch");
    assert_eq!(watch["schema"], "canvas-cli/watch@1");
    assert_eq!(watch["output"]["form"], "envelope");
    assert_eq!(watch["output"]["flag"], "--jsonl");
    assert_eq!(watch["output"]["refuses"][0], "--json");
    let comment = watch["output"]["$comment"].as_str().unwrap_or_default();
    assert!(
        comment.contains("--jsonl` is the machine-readable form"),
        "{comment}"
    );
    assert!(
        comment.contains("`--json` is refused with exit 2"),
        "{comment}"
    );
    // The closing summary is still a §7 envelope with both branches.
    assert_eq!(
        watch["envelope"]["oneOf"][1]["properties"]["schema"]["const"],
        "canvas-cli/error@1"
    );
}

/// The generated schema must accept the document the command actually prints.
#[test]
fn the_version_schema_accepts_the_version_envelope() {
    let schema: serde_json::Value = serde_json::from_slice(
        &canvas()
            .args(["schema", "version"])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .expect("schema document");
    let envelope: serde_json::Value = serde_json::from_slice(
        &canvas()
            .args(["version", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .expect("version envelope");

    let branch = &schema["envelope"]["oneOf"][0];
    let properties = branch["properties"].as_object().expect("properties");
    for field in branch["required"].as_array().expect("required") {
        let name = field.as_str().expect("field name");
        assert!(
            envelope.get(name).is_some(),
            "the envelope has no {name}: {envelope}"
        );
    }
    let result = properties["result"]["properties"]
        .as_object()
        .expect("result properties");
    for name in result.keys() {
        assert!(
            envelope["result"].get(name).is_some(),
            "version@1 result has no {name}"
        );
    }
}

#[test]
fn json_is_a_usage_error_and_an_unknown_command_is_a_resolution_error() {
    canvas()
        .args(["schema", "todo", "--json"])
        .assert()
        .code(2)
        .stdout("");
    canvas().args(["schema", "nonesuch"]).assert().code(6);
    // The operand is required unless `--list` asks for the registry.
    canvas().arg("schema").assert().code(2);
}
