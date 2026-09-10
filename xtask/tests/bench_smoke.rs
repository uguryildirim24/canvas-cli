//! `xtask bench` end to end, in the shape CI uses.
//!
//! The run is short (`--runs 2`) and enforces no target (`--no-fail`): a shared
//! runner is not a machine whose timings mean anything. What is checked is that
//! the whole path works — the fixture set serves, the cache primes, the release
//! binary runs, a download loads the server, and the report comes out complete.
//!
//! The report goes to a temporary file. A test must not rewrite the tracked
//! `docs/bench.md`, whose numbers belong to a deliberate run.

use std::path::Path;
use std::process::Command;

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask has a parent")
}

#[test]
fn a_short_run_measures_every_metric_and_writes_a_complete_report() {
    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("bench.md");

    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(workspace_root())
        .args(["bench", "--runs", "2", "--no-fail"])
        .arg("--doc")
        .arg(&doc)
        .output()
        .expect("run xtask bench");
    assert!(
        output.status.success(),
        "bench failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let table = String::from_utf8_lossy(&output.stdout);
    // Three metrics, each idle and under a download.
    for label in [
        "cached todo, first output",
        "cached todo, full run",
        "cold start",
    ] {
        assert_eq!(
            table.matches(label).count(),
            2,
            "{label} was not measured twice:\n{table}"
        );
    }
    assert_eq!(table.matches("idle").count(), 3, "{table}");
    assert_eq!(table.matches("download").count(), 3, "{table}");

    let report = std::fs::read_to_string(&doc).expect("report written");
    for section in [
        "# Benchmarks",
        "## Targets and measurements",
        "## Method",
        "## Limitations",
    ] {
        assert!(report.contains(section), "{section} missing:\n{report}");
    }
    for fact in [
        "| Date |",
        "| Commit |",
        "| Machine |",
        "| Fixture set | `bench-5` |",
        "| Runs per metric | 2 |",
    ] {
        assert!(report.contains(fact), "{fact} missing:\n{report}");
    }
    // Every SPEC section 13 target is named with its number.
    for target in ["50", "150", "250", "400"] {
        assert!(report.contains(target), "target {target} missing");
    }
    // Six measured rows.
    assert_eq!(
        report
            .lines()
            .filter(|l| l.contains("| idle |") || l.contains("| download |"))
            .count(),
        6,
        "{report}"
    );

    // The tracked report was not touched.
    let tracked = workspace_root().join("docs").join("bench.md");
    assert!(!report.contains("Runs per metric | 3"));
    assert!(tracked.exists(), "the tracked report should still be there");
}

#[test]
fn an_unknown_fixture_set_is_refused_rather_than_generated() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(workspace_root())
        .args([
            "bench",
            "--fixture",
            "no-such-set",
            "--runs",
            "1",
            "--no-fail",
        ])
        .output()
        .expect("run xtask bench");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no-such-set"), "{stderr}");
    assert!(
        !workspace_root()
            .join("crates/canvas-api/tests/fixtures/no-such-set")
            .exists()
    );
}
