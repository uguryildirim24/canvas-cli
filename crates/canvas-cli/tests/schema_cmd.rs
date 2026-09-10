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
        let (command, id) = line.split_once('\t').expect("tab-separated");
        assert!(id.starts_with("canvas-cli/"), "{line}");
        assert!(id.ends_with("@1"), "{line}");
        let base = id["canvas-cli/".len()..id.len() - 2].replace('_', " ");
        // A schema with several result shapes lists one row per shape, named
        // after the subcommand that prints it: `receipts show`, not a second
        // indistinguishable `receipts`.
        assert!(
            command == base
                || command
                    .strip_prefix(&base)
                    .is_some_and(|v| v.starts_with(' ')),
            "{line} does not name {base}"
        );
        // Every row must name a different command, or a caller cannot ask for
        // the shape it wants.
        assert!(seen.insert(command.to_owned()), "{command} is listed twice");
    }
    for expected in [
        "todo\tcanvas-cli/todo@1",
        "calendar\tcanvas-cli/calendar@1",
        "error\tcanvas-cli/error@1",
        "auth status\tcanvas-cli/auth_status@1",
        "receipts list\tcanvas-cli/receipts@1",
        "receipts show\tcanvas-cli/receipts@1",
        "receipts acknowledge\tcanvas-cli/receipts@1",
    ] {
        assert!(stdout.contains(expected), "missing {expected}: {stdout}");
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
