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
/// this list against §21.2, and this file keeps the skill honest without
/// making the binary's private modules public.
const CATALOG: &[&str] = &[
    "courses.list",
    "course.get",
    "todo.list",
    "assignments.list",
    "assignment.get",
    "grades.get",
    "files.list",
    "modules.list",
    "pages.list",
    "page.get",
    "syllabus.get",
    "announcements.list",
    "announcement.get",
    "discussions.list",
    "discussion.get",
    "inbox.list",
    "inbox.get",
    "inbox.unread_count",
    "calendar.list",
    "submission.get",
    "receipts.list",
    "receipts.show",
];

/// The 21 names the catalog no longer serves (§19 item 48).
///
/// A skill that still named one would send a host to a tool that is not
/// there. Each of these is a `canvas` command now, and the workflows say so.
const REMOVED: &[&str] = &[
    "sync.run",
    "download.plan",
    "download.run",
    "submission.prepare",
    "submission.execute",
    "submission.reconcile",
    "discussion.reply.prepare",
    "discussion.reply.execute",
    "inbox.send.prepare",
    "inbox.send.execute",
    "inbox.reply.prepare",
    "inbox.reply.execute",
    "operation.status",
    "operation.reconcile",
    "receipts.acknowledge",
    "open.url",
    "context.attach",
    "context.here",
    "context.detach",
    "context.note",
    "context.follow",
];

/// The four workflows whose subject is a write, and the command each one
/// must route through now that no tool can perform it.
///
/// A workflow that describes a write without naming the command behind it
/// leaves a model with no way to do what the user asked.
const WRITE_WORKFLOWS: &[(&str, &[&str])] = &[
    ("prepare-and-submit.md", &["canvas submit"]),
    (
        "reply-and-message-with-approval.md",
        &[
            "canvas discussion reply",
            "canvas inbox send",
            "canvas inbox reply",
            "canvas operation status",
        ],
    ),
    (
        "reconcile-an-unknown-outcome.md",
        &["canvas submission reconcile", "canvas receipts acknowledge"],
    ),
    ("download-course-files.md", &["canvas download"]),
];

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

/// Whether a token is shaped like a tool name: `<noun>.<verb>`, or the
/// three-part form the M8-b writes use (`inbox.send.prepare`).
fn tool_shaped(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    // Two parts, or three when the last one is a plan step: that is the only
    // three-part shape the catalog has, and it keeps a dotted field path such
    // as `result.details.reason` from being read as a tool name.
    let shape = match parts.as_slice() {
        [_, _] => true,
        [_, _, last] => matches!(*last, "prepare" | "execute"),
        _ => false,
    };
    if !shape {
        return false;
    }
    let word = |part: &str| {
        !part.is_empty()
            && part.starts_with(|c: char| c.is_ascii_lowercase())
            && part.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    };
    parts.iter().all(|part| word(part))
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
            "reply-and-message-with-approval.md",
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

/// No workflow names a tool the catalog dropped (§19 item 48).
///
/// `every_tool_the_skill_names_exists` catches a tool-shaped token; this
/// catches the name anywhere in the file, including prose, so a sentence
/// left behind cannot tell a model to call something that is gone.
#[test]
fn no_workflow_names_a_removed_tool() {
    for path in files() {
        let text = std::fs::read_to_string(&path).expect("read a skill file");
        for name in REMOVED {
            assert!(
                !text.contains(name),
                "{} still names {name}, which left the catalog",
                path.display()
            );
        }
    }
}

/// Every workflow whose subject is a write routes through the command line.
///
/// The MCP catalog cannot perform any of them, so a workflow that does not
/// name the command behind it leaves a model stuck.
#[test]
fn every_write_workflow_routes_through_the_cli() {
    for (file, commands) in WRITE_WORKFLOWS {
        let path = skill_dir().join(file);
        let text = std::fs::read_to_string(&path).expect("the workflow ships");
        for command in *commands {
            assert!(text.contains(command), "{file} does not name `{command}`");
        }
    }
}

/// The reply workflow still carries the course-policy boundary REPORT §3.5
/// states in plain words, whichever surface performs the write.
#[test]
fn the_reply_workflow_states_the_course_policy_boundary() {
    let workflow = skill_dir().join("reply-and-message-with-approval.md");
    let text = std::fs::read_to_string(&workflow).expect("the reply workflow ships");
    for required in [
        "not permission for AI-generated academic work",
        "Never write a placeholder",
        "not_observable",
        "Never say",
    ] {
        assert!(
            text.contains(required),
            "the reply workflow does not state: {required}"
        );
    }
}

/// SKILL.md says the catalog is read-only, and says it where a model reads
/// it before anything else.
#[test]
fn the_skill_states_that_the_catalog_is_read_only() {
    let text = std::fs::read_to_string(skill_dir().join("SKILL.md")).expect("SKILL.md ships");
    for required in [
        "The MCP catalog is read-only",
        "Never pass `--yes`",
        "22 tools",
    ] {
        assert!(text.contains(required), "SKILL.md never states: {required}");
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
