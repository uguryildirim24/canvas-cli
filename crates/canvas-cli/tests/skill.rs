//! The shipped skill must describe the product that exists.
//!
//! `canvas mcp` serves one tool (§21.2), so a workflow's steps are `canvas`
//! commands. A skill that still named a tool would send a host down a path
//! that fails, and a workflow that names no command leaves a model with no
//! way to do what the user asked. This file diffs the skill against both.

use std::path::{Path, PathBuf};

/// The only tool `canvas mcp` serves (§19 item 50).
const TOOL: &str = "getclitools";

/// Every tool name the server used to serve, and serves no longer.
///
/// The 22 reads M9-b removed and the 21 writes M9 removed. A workflow that
/// still names one of them — in a call block or in a sentence — tells a model
/// to call something that answers `METHOD_NOT_FOUND`.
const GONE: &[&str] = &[
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

/// What each workflow must route through, now that every step is a command.
///
/// The reads are here as well as the writes: M9 gave the four write workflows
/// their commands, and M9-b gives the reads theirs, because there is no tool
/// left to read with.
const WORKFLOWS: &[(&str, &[&str])] = &[
    (
        "organize-the-week.md",
        &[
            "canvas courses",
            "canvas course ",
            "canvas todo",
            "canvas calendar",
            "canvas announcements",
            "canvas announcement ",
            "canvas grades",
            "canvas inbox unread-count",
            "canvas inbox --scope",
            "canvas inbox show",
            "canvas discussions",
            "canvas discussion ",
            "canvas sync",
        ],
    ),
    (
        "read-an-assignment.md",
        &[
            "canvas assignments",
            "canvas assignment ",
            "canvas submission ",
            "canvas grades",
            "canvas syllabus",
            "canvas pages",
            "canvas page ",
            "canvas discussions",
            "canvas discussion ",
        ],
    ),
    (
        "prepare-and-submit.md",
        &[
            "canvas submit",
            "canvas submission ",
            "canvas receipts list",
            "canvas receipts show",
        ],
    ),
    (
        "reply-and-message-with-approval.md",
        &[
            "canvas discussion ",
            "canvas inbox show",
            "canvas discussion reply",
            "canvas inbox send",
            "canvas inbox reply",
            "canvas operation status",
            "canvas operation reconcile",
        ],
    ),
    (
        "reconcile-an-unknown-outcome.md",
        &[
            "canvas receipts list",
            "canvas receipts show",
            "canvas submission ",
            "canvas submission reconcile",
            "canvas receipts acknowledge",
        ],
    ),
    (
        "download-course-files.md",
        &["canvas files", "canvas modules", "canvas download"],
    ),
    (
        "take-a-quiz.md",
        &[
            "canvas quizzes",
            "canvas new-quizzes",
            "canvas new-quiz",
            "canvas quiz ",
            "canvas quiz questions",
            "canvas quiz submit",
            "canvas operation status",
            "canvas operation reconcile",
        ],
    ),
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
            "take-a-quiz.md",
        ]
    );
}

/// Nothing in the skill names a tool that left the server (§19 items 48, 50).
///
/// The whole file is searched, prose included, so a sentence left behind
/// cannot tell a model to call something that is gone.
#[test]
fn no_file_names_a_removed_tool() {
    for path in files() {
        let text = std::fs::read_to_string(&path).expect("read a skill file");
        for name in GONE {
            assert!(
                !text.contains(name),
                "{} still names {name}, which the server no longer serves",
                path.display()
            );
        }
    }
}

/// Every workflow routes through the command line, reads as well as writes.
#[test]
fn every_workflow_routes_through_the_cli() {
    for (file, commands) in WORKFLOWS {
        let path = skill_dir().join(file);
        let text = std::fs::read_to_string(&path).expect("the workflow ships");
        for command in *commands {
            assert!(text.contains(command), "{file} does not name `{command}`");
        }
        // Every workflow ends in a block a model can copy.
        assert!(
            text.contains("```sh\ncanvas "),
            "{file} carries no runnable command block"
        );
    }
}

/// The reply workflow states the user-authorization stance of SPEC §19
/// item 51 in plain words, whichever surface performs the write.
#[test]
fn the_reply_workflow_states_the_course_policy_boundary() {
    let workflow = skill_dir().join("reply-and-message-with-approval.md");
    let text = std::fs::read_to_string(&workflow).expect("the reply workflow ships");
    for required in [
        "The user decides what help is allowed",
        "Never lecture about academic integrity",
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

/// SKILL.md says what the MCP surface is, where a model reads it first.
#[test]
fn the_skill_states_the_one_tool_surface() {
    let text = std::fs::read_to_string(skill_dir().join("SKILL.md")).expect("SKILL.md ships");
    for required in [
        "one tool, `getclitools`",
        "no resource, and no subscription",
        "Never pass `--yes`",
        "run `canvas <command> ...` yourself",
    ] {
        assert!(text.contains(required), "SKILL.md never states: {required}");
    }
    // One tool means one tool name in the whole package.
    for path in files() {
        let text = std::fs::read_to_string(&path).expect("read a skill file");
        for line in text.lines() {
            for (index, span) in line.split('`').enumerate() {
                let dotted = span.split('.').count() == 2
                    && span.split('.').all(|part| {
                        !part.is_empty() && part.chars().all(|c| c.is_ascii_lowercase())
                    });
                assert!(
                    !(index % 2 == 1 && dotted),
                    "{} names {span}, which is not {TOOL}",
                    path.display()
                );
            }
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
