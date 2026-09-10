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
    let commands = help.split("\nOptions:").next().unwrap();
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
    ] {
        assert!(
            commands
                .lines()
                .any(|line| line.split_whitespace().next() == Some(name)),
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
    let result = Command::cargo_bin("canvas")
        .unwrap()
        .args(args)
        .assert()
        .code(1);
    if args.contains(&"--json") {
        let v: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
        assert_eq!(v["schema"], "canvas-cli/error@1");
        assert_eq!(v["result"]["message"], "not implemented yet");
    } else {
        result.stdout("").stderr("not implemented yet\n");
    }
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
fn submission_accepts_options_between_operands() {
    for options in [
        vec!["--fresh"],
        vec!["--offline"],
        vec!["--json"],
        vec!["--profile", "school"],
        vec!["--color", "never"],
        vec!["-q", "-v"],
        vec!["--history"],
    ] {
        let mut args = vec!["submission", "chem"];
        args.extend(options);
        args.push("123");
        assert_stub(&args);
    }
    assert_usage_error(&["submission", "chem", "--fresh", "123", "--offline"]);
    assert_usage_error(&["--offline", "submission", "chem", "--fresh", "123"]);
    assert_usage_error(&["submission", "chem", "--fresh"]);
    assert_usage_error(&["submission", "chem", "--fresh", "123", "extra"]);
    // Escape a subcommand name when it follows an option.
    assert_stub(&["submission", "chem", "--fresh", "--", "verify"]);
    assert_stub(&["submission", "--", "verify", "123"]);
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

#[test]
fn command_choices_reject_invalid_combinations() {
    for args in [
        vec!["download"],
        vec!["download", "chem", "--all-courses"],
        vec!["download", "chem", "--file"],
        vec!["download", "chem", "--file", "not-an-id"],
        vec!["assignments", "chem", "--bucket", "typo"],
        vec!["submit", "chem", "123"],
    ] {
        assert_usage_error(&args);
    }
    let modes = [
        ("--file", "a.txt"),
        ("--text", "-"),
        ("--html", "a.html"),
        ("--url", "https://example.test"),
    ];
    for (index, first) in modes.iter().enumerate() {
        for second in &modes[index + 1..] {
            assert_usage_error(&[
                "submit", "chem", "123", first.0, first.1, second.0, second.1,
            ]);
        }
    }
}

#[test]
fn command_choices_accept_documented_forms() {
    for bucket in [
        "open",
        "upcoming",
        "overdue",
        "past",
        "undated",
        "unsubmitted",
        "ungraded",
        "future",
        "all",
    ] {
        assert_stub(&["assignments", "chem", "--bucket", bucket]);
    }
    for args in [
        vec!["download", "chem"],
        vec!["download", "--all-courses"],
        vec!["download", "chem", "--file", "1", "2"],
        vec![
            "download",
            "--all-courses",
            "--file",
            "1",
            "--file",
            "2",
            "3",
        ],
        vec![
            "submit", "chem", "123", "--file", "a.txt", "--file", "b.txt",
        ],
        vec!["submit", "chem", "123", "--text", "-"],
        vec!["submit", "chem", "123", "--html", "a.html"],
        vec!["submit", "chem", "123", "--url", "https://example.test"],
        vec![
            "submit",
            "https://canvas.example.test/courses/1/assignments/2",
            "--file",
            "a.txt",
        ],
    ] {
        assert_stub(&args);
    }
}

#[test]
fn raw_output_rejects_json_at_every_command_level() {
    for command in [
        vec!["completions", "bash"],
        vec!["auth", "token", "--reveal"],
        vec!["config", "edit"],
        vec!["calendar", "--ics", "-"],
        vec!["receipts", "export", "receipt-1", "--out", "-"],
    ] {
        for position in 0..=command.len() {
            // Do not insert a flag between a value-taking option and its value.
            if position > 0 && ["--ics", "--out"].contains(&command[position - 1]) {
                continue;
            }
            let mut args = command.clone();
            args.insert(position, "--json");
            assert_usage_error(&args);
        }
    }
}

#[test]
fn nonraw_variants_continue_to_accept_json() {
    for args in [
        vec!["auth", "token", "--json"],
        vec!["calendar", "--ics", "calendar.ics", "--json"],
        vec!["receipts", "export", "receipt-1", "--json"],
        vec![
            "receipts",
            "export",
            "receipt-1",
            "--out",
            "receipt.json",
            "--json",
        ],
    ] {
        assert_stub(&args);
    }
}

#[test]
fn nested_help_lists_registered_commands() {
    for (parent, names) in [
        ("auth", vec!["login", "status", "logout", "token"]),
        ("identity", vec!["list", "remove"]),
        ("submission", vec!["verify", "reconcile"]),
        ("receipts", vec!["list", "show", "export", "acknowledge"]),
        ("cache", vec!["stats", "clear", "path"]),
        ("config", vec!["path", "edit", "get", "set"]),
        ("alias", vec!["set", "list", "remove"]),
        ("open", vec!["assignment", "file", "announcement"]),
    ] {
        let assert = Command::cargo_bin("canvas")
            .unwrap()
            .args([parent, "--help"])
            .assert()
            .success();
        let help = String::from_utf8_lossy(&assert.get_output().stdout);
        let commands = help.split("\nOptions:").next().unwrap();
        for name in names {
            assert!(
                commands
                    .lines()
                    .any(|line| line.split_whitespace().next() == Some(name)),
                "help missing {parent} {name}: {help}"
            );
        }
    }
}

#[test]
fn every_v1_stub_is_callable() {
    let cases: &[&[&str]] = &[
        &["auth", "login"],
        &["auth", "status"],
        &["auth", "logout"],
        &["auth", "token", "--reveal"],
        &["identity", "list"],
        &["identity", "remove", "identity-1"],
        &["todo"],
        &["assignments", "chem"],
        &["assignment", "chem", "123"],
        &[
            "assignment",
            "https://canvas.example.test/courses/1/assignments/2",
        ],
        &["submit", "chem", "123", "--file", "a.txt"],
        &["submission", "chem", "123", "--history"],
        &["submission", "verify", "receipt-1"],
        &["submission", "reconcile", "journal-1"],
        &["receipts", "list"],
        &["receipts", "show", "receipt-1"],
        &["receipts", "export", "receipt-1"],
        &["receipts", "acknowledge", "journal-1"],
        &["grades"],
        &["download", "chem"],
        &["announcements"],
        &["announcement", "chem", "123"],
        &[
            "announcement",
            "https://canvas.example.test/courses/1/discussion_topics/2",
        ],
        &["calendar"],
        &["open", "chem"],
        &["open", "https://canvas.example.test/courses/1"],
        &["open", "assignment", "chem", "123"],
        &["open", "file", "123"],
        &["open", "announcement", "chem", "123"],
        &["config", "path"],
        &["config", "edit"],
        &["config", "get", "key"],
        &["config", "set", "key", "value"],
        &["doctor"],
    ];
    for args in cases {
        assert_stub(args);
    }
}

/// M1-b commands need an identity; without one they exit 3 with an error envelope.
#[test]
fn m1b_commands_exit_auth_without_identity() {
    let empty = tempfile::TempDir::new().unwrap();
    for args in [
        vec!["courses"],
        vec!["course", "chem"],
        vec!["alias", "list"],
        vec!["alias", "set", "chem", "123"],
        vec!["alias", "remove", "chem"],
        vec!["sync"],
        vec!["cache", "stats"],
        vec!["cache", "clear"],
        vec!["cache", "path"],
        vec!["files", "chem"],
        vec!["modules", "chem"],
    ] {
        let assert = Command::cargo_bin("canvas")
            .unwrap()
            .env("CANVAS_DATA_ROOT", empty.path())
            .env_remove("CANVAS_IDENTITY_KEY")
            .env_remove("CANVAS_TOKEN")
            .env_remove("HOME")
            .args(&args)
            .args(["--json", "--color", "never"])
            .assert()
            .code(3);
        let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
        assert!(
            stdout.contains("\"schema\":\"canvas-cli/error@1\"")
                || stdout.contains("\"schema\": \"canvas-cli/error@1\""),
            "args={args:?} stdout={stdout}"
        );
        assert!(
            stdout.contains("\"code\":\"auth\"") || stdout.contains("\"code\": \"auth\""),
            "args={args:?} stdout={stdout}"
        );
    }
}

#[test]
fn completions_support_every_documented_shell() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let assert = Command::cargo_bin("canvas")
            .unwrap()
            .args(["completions", shell])
            .assert()
            .success()
            .stderr("");
        let output = String::from_utf8_lossy(&assert.get_output().stdout);
        assert!(
            output.contains("canvas"),
            "missing canvas completion for {shell}"
        );
    }
    assert_usage_error(&["completions", "unknown-shell"]);
}

#[test]
fn version_json_is_a_single_identity_free_envelope() {
    let result = Command::cargo_bin("canvas")
        .unwrap()
        .args(["version", "--json", "--color", "always"])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(v["schema"], "canvas-cli/version@1");
    assert_eq!(v["result"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(v["profile"].is_null());
    assert!(v["identity"].is_null());
    assert!(v["result"]["target"].as_str().unwrap().contains('-'));
}
