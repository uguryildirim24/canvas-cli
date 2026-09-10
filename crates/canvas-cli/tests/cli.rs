use assert_cmd::Command;

#[test]
fn version_prints_crate_version() {
    let assert = Command::cargo_bin("canvas")
        .unwrap()
        .arg("version")
        .assert()
        .success();
    let output = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        output.contains(env!("CARGO_PKG_VERSION")),
        "stdout={output:?}"
    );
}

#[test]
fn todo_is_stub() {
    Command::cargo_bin("canvas")
        .unwrap()
        .arg("todo")
        .assert()
        .code(1)
        .stderr("not implemented yet\n");
}

#[test]
fn help_lists_v1_commands() {
    let assert = Command::cargo_bin("canvas")
        .unwrap()
        .arg("--help")
        .assert()
        .success();
    let help = String::from_utf8_lossy(&assert.get_output().stdout);
    for name in [
        "auth",
        "identity",
        "courses",
        "course",
        "todo",
        "assignments",
        "assignment",
        "submit",
        "submission",
        "receipts",
        "grades",
        "files",
        "download",
        "modules",
        "announcements",
        "announcement",
        "calendar",
        "open",
        "sync",
        "cache",
        "config",
        "alias",
        "doctor",
        "completions",
        "version",
        "login",
        "verify",
        "reconcile",
        "acknowledge",
    ] {
        assert!(
            help.contains(name),
            "help missing command {name:?}:\n{help}"
        );
    }
}

#[test]
fn fresh_conflicts_with_offline() {
    let assert = Command::cargo_bin("canvas")
        .unwrap()
        .args(["todo", "--fresh", "--offline"])
        .assert()
        .code(2);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("--offline"),
        "stderr={stderr:?}"
    );
}

fn assert_stub(args: &[&str]) {
    Command::cargo_bin("canvas")
        .unwrap()
        .args(args)
        .assert()
        .code(1)
        .stdout("")
        .stderr("not implemented yet\n");
}

fn assert_usage_error(args: &[&str]) {
    let assert = Command::cargo_bin("canvas")
        .unwrap()
        .args(args)
        .assert()
        .code(2)
        .stdout("");
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("error:"), "stderr={stderr:?}");
    assert!(
        stderr.contains("Usage:") || stderr.contains("try '--help'"),
        "stderr={stderr:?}"
    );
}

#[test]
fn mixed_commands_accept_typed_and_positional_forms() {
    for args in [
        vec!["submission", "chem", "123", "--history", "--fresh"],
        vec!["submission", "verify", "receipt-1", "--fresh"],
        vec!["submission", "--fresh", "verify", "receipt-1"],
        vec!["submission", "chem", "verify"],
        vec![
            "submission",
            "reconcile",
            "journal-1",
            "--assume-not-submitted",
        ],
        vec!["open", "chem", "--offline"],
        vec!["open", "https://canvas.example.test/courses/1", "--offline"],
        vec!["open", "assignment", "chem", "123"],
        vec!["open", "file", "123"],
        vec!["open", "--offline", "file", "123"],
        vec!["open", "announcement", "chem", "123"],
        vec!["open", "--", "assignment"],
    ] {
        assert_stub(&args);
    }
}

#[test]
fn mixed_commands_reject_missing_extra_and_unknown_arguments() {
    for args in [
        vec!["submission"],
        vec!["submission", "chem"],
        vec!["submission", "chem", "123", "extra"],
        vec!["submission", "chem", "123", "extra", "extra"],
        vec!["submission", "chem", "123", "--bogus"],
        vec!["submission", "chem", "123", "--profile"],
        vec!["submission", "verify", "receipt-1", "--history"],
        vec!["submission", "--history", "verify", "receipt-1"],
        vec!["open"],
        vec!["open", "chem", "extra"],
        vec!["open", "chem", "--bogus"],
        vec!["open", "chem", "--color", "invalid"],
        vec!["open", "file"],
        vec!["open", "chem", "file", "123"],
        vec!["submission", "chem", "123", "verify", "receipt-1"],
    ] {
        assert_usage_error(&args);
    }
}

#[test]
fn mixed_commands_enforce_global_conflicts_after_operands() {
    for command in [
        vec!["todo"],
        vec!["submission", "chem", "123"],
        vec!["submission", "verify", "receipt-1"],
        vec!["submission", "reconcile", "journal-1"],
        vec!["open", "chem"],
        vec!["open", "assignment", "chem", "123"],
        vec!["open", "file", "123"],
        vec!["open", "announcement", "chem", "123"],
    ] {
        let mut args = command.clone();
        args.extend(["--fresh", "--offline"]);
        assert_usage_error(&args);

        let mut args = vec!["--fresh"];
        args.extend(command);
        args.push("--offline");
        assert_usage_error(&args);
    }
}

#[test]
fn mixed_command_help_lists_real_operands() {
    for (command, expected) in [
        ("submission", vec!["COURSE", "ASSIGNMENT", "--history"]),
        ("open", vec!["TARGET"]),
    ] {
        let assert = Command::cargo_bin("canvas")
            .unwrap()
            .args([command, "--help"])
            .assert()
            .success();
        let help = String::from_utf8_lossy(&assert.get_output().stdout);
        for operand in expected {
            assert!(help.contains(operand), "help={help}");
        }
    }
}
