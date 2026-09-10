//! The shipped skill and the tool catalog must name the same tools.
//!
//! A skill that names a tool the server does not have sends a host down a
//! path that fails, and a tool no workflow mentions is a surface nobody was
//! told about. This test diffs the two in both directions.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The catalog `canvas mcp` serves, in the server's order.
///
/// `crates/canvas-cli/src/mcp/catalog.rs` is the source; its own test pins
/// this list against REPORT §3.2, and this file keeps the skill honest
/// without making the binary's private modules public.
const CATALOG: &[&str] = &[
    "courses.list",
    "course.get",
    "todo.list",
    "assignments.list",
    "assignment.get",
    "grades.get",
    "files.list",
    "modules.list",
    "announcements.list",
    "announcement.get",
    "calendar.list",
    "submission.get",
    "receipts.list",
    "receipts.show",
    "sync.run",
    "download.plan",
    "download.run",
    "submission.prepare",
    "submission.execute",
    "submission.reconcile",
    "receipts.acknowledge",
    "open.url",
];

/// The two tools that can send anything to Canvas.
///
/// They belong to one workflow, because a submission is one procedure: freeze
/// a plan, show it to a person, then execute what was approved.
const SUBMISSION: &[&str] = &["submission.prepare", "submission.execute"];

fn skill_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("skill/canvas-cli")
}

fn files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(skill_dir())
        .expect("the skill ships in the repository")
        .map(|entry| entry.expect("read the skill directory").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    files.sort();
    files
}

/// Whether a token is shaped like a tool name: `<noun>.<verb>`.
fn tool_shaped(token: &str) -> bool {
    let mut parts = token.split('.');
    let (Some(head), Some(tail), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let word = |part: &str| {
        !part.is_empty()
            && part.starts_with(|c: char| c.is_ascii_lowercase())
            && part.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    };
    word(head) && word(tail)
}

/// Every tool name the skill names.
///
/// A skill names a tool two ways: as a whole inline code span, and as the
/// first token of a call line in a fenced block. Nothing else counts, so a
/// path or a config key that happens to carry a dot is not mistaken for one.
fn mentioned() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for path in files() {
        let text = std::fs::read_to_string(&path).expect("read a skill file");
        let mut fenced = false;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                if let Some(first) = line.split_whitespace().next()
                    && tool_shaped(first)
                {
                    names.insert(first.to_owned());
                }
                continue;
            }
            for (index, span) in line.split('`').enumerate() {
                if index % 2 == 1 && tool_shaped(span) {
                    names.insert(span.to_owned());
                }
            }
        }
        assert!(!fenced, "{} has an unclosed code fence", path.display());
    }
    names
}

#[test]
fn the_skill_ships_one_file_per_workflow() {
    let names: Vec<String> = files()
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "SKILL.md",
            "download-course-files.md",
            "organize-the-week.md",
            "prepare-and-submit.md",
            "read-an-assignment.md",
            "reconcile-an-unknown-outcome.md",
        ]
    );
}

#[test]
fn every_tool_the_skill_names_exists() {
    let known: BTreeSet<&str> = CATALOG.iter().copied().collect();
    for name in mentioned() {
        assert!(known.contains(name.as_str()), "the skill names {name}");
    }
}

#[test]
fn every_catalog_tool_appears_in_the_skill() {
    let mentioned = mentioned();
    let missing: Vec<&str> = CATALOG
        .iter()
        .copied()
        .filter(|name| !mentioned.contains(*name))
        .collect();
    assert!(missing.is_empty(), "no workflow mentions {missing:?}");
}

/// A submission is described in one place, so no other workflow can imply
/// that it sends anything.
#[test]
fn the_submission_tools_are_only_named_by_the_approval_workflow() {
    let approval = skill_dir().join("prepare-and-submit.md");
    let text = std::fs::read_to_string(&approval).expect("the approval workflow ships");
    for name in SUBMISSION {
        assert!(text.contains(name), "{name} has no workflow");
    }
    for path in files() {
        if path == approval {
            continue;
        }
        let other = std::fs::read_to_string(&path).expect("read a skill file");
        for name in SUBMISSION {
            assert!(
                !other.contains(name),
                "{} names {name} outside the approval workflow",
                path.display()
            );
        }
    }
}

/// The skill must state the contract it asks a host to rely on.
#[test]
fn the_skill_states_the_envelope_and_the_exit_codes() {
    let text = std::fs::read_to_string(skill_dir().join("SKILL.md")).expect("SKILL.md ships");
    for required in [
        "outcome",
        "exit",
        "freshness",
        "partial",
        "warnings",
        "canvas schema",
        "identity",
    ] {
        assert!(
            text.contains(required),
            "SKILL.md never mentions {required}"
        );
    }
    // Every §14 code an agent can meet, with a recovery line.
    for code in [
        "| 2 |", "| 3 |", "| 6 |", "| 7 |", "| 8 |", "| 9 |", "| 12 |", "| 13 |",
    ] {
        assert!(text.contains(code), "no recovery line for {code}");
    }
    // The three hosts the package documents.
    for host in ["Claude Code", "Codex", "Cursor"] {
        assert!(text.contains(host), "no MCP setup for {host}");
    }
}

/// The release archives carry the skill (§17), and the README points at it.
#[test]
fn the_skill_is_shipped_and_referenced() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dist = std::fs::read_to_string(root.join("dist-workspace.toml")).expect("dist config");
    assert!(
        dist.contains("\"skill\""),
        "the release archives do not carry the skill"
    );
    let readme = std::fs::read_to_string(root.join("README.md")).expect("README");
    assert!(
        readme.contains("skill/canvas-cli"),
        "the README does not point at the skill"
    );
}
