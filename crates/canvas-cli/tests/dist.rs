//! `completions` and `version` (§5, class A) against the packaging contract.

use std::process::Command;

use assert_cmd::cargo::cargo_bin;

fn canvas() -> Command {
    let mut cmd = Command::new(cargo_bin!("canvas"));
    cmd.env_remove("CANVAS_TOKEN")
        .env_remove("CANVAS_HOST")
        .env_remove("CANVAS_PROFILE");
    cmd
}

#[test]
fn every_shell_prints_the_same_script_the_release_archives_carry() {
    for shell in canvas_cli::dist::SHELLS {
        let name = shell.to_string();
        let out = canvas().args(["completions", &name]).output().unwrap();
        assert!(out.status.success(), "completions {name}: {:?}", out.status);
        assert!(out.stderr.is_empty(), "completions {name} wrote to stderr");

        let mut expected = Vec::new();
        canvas_cli::dist::write_completions(*shell, &mut expected);
        assert!(!expected.is_empty(), "no script generated for {name}");
        assert_eq!(
            out.stdout, expected,
            "`canvas completions {name}` differs from the generated asset"
        );
    }
}

#[test]
fn each_script_is_written_for_its_own_shell() {
    for (shell, marker) in [
        ("bash", "COMPREPLY"),
        ("zsh", "#compdef canvas"),
        ("fish", "complete -c canvas"),
        ("powershell", "Register-ArgumentCompleter"),
        ("elvish", "edit:completion:arg-completer"),
    ] {
        let out = canvas().args(["completions", shell]).output().unwrap();
        let script = String::from_utf8_lossy(&out.stdout);
        assert!(
            script.contains(marker),
            "{shell} script is missing {marker:?}"
        );
    }
}

#[test]
fn completions_reject_json_with_exit_2() {
    let out = canvas()
        .args(["completions", "bash", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "raw-output refusal wrote to stdout");
}

#[test]
fn version_reports_the_commit_the_binary_was_built_from() {
    let out = canvas().args(["version", "--json"]).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], "canvas-cli/version@1");
    assert_eq!(v["result"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["result"]["target"], env!("CANVAS_BUILD_TARGET"));

    // The build script stamps the commit; both targets see the same value.
    match option_env!("CANVAS_COMMIT") {
        Some(commit) => {
            assert_eq!(v["result"]["commit"], commit);
            assert!(
                commit.len() >= 7 && commit.chars().all(|c| c.is_ascii_hexdigit()),
                "commit {commit:?} is not a git hash"
            );
        }
        None => assert!(
            v["result"]["commit"].is_null(),
            "no commit was stamped, so `commit` must be null"
        ),
    }
}

#[test]
fn version_human_output_names_the_version_commit_and_target() {
    let out = canvas().arg("version").output().unwrap();
    assert!(out.status.success());
    let line = String::from_utf8_lossy(&out.stdout);
    assert!(line.contains(env!("CARGO_PKG_VERSION")), "{line}");
    assert!(line.contains(env!("CANVAS_BUILD_TARGET")), "{line}");
    if let Some(commit) = option_env!("CANVAS_COMMIT") {
        assert!(line.contains(commit), "{line}");
    }
}
