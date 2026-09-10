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
