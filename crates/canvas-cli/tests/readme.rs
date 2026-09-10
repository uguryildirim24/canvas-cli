//! The README command table is the user-facing copy of the clap command list.
//! These tests fail when one drifts from the other.

use std::collections::BTreeSet;

const README: &str = include_str!("../../../README.md");

/// Every command path a user can actually invoke.
///
/// A command with subcommands and no operands of its own (`canvas auth`) is a
/// group, not a command, and gets no row. `submission` and `open` take both
/// operands and subcommands, so they get a row and their subcommands do too.
fn clap_commands() -> BTreeSet<String> {
    fn walk(cmd: &clap::Command, path: &str, out: &mut BTreeSet<String>) {
        let subs: Vec<_> = cmd
            .get_subcommands()
            .filter(|s| s.get_name() != "help" && !s.is_hide_set())
            .collect();
        let has_operands = cmd.get_positionals().next().is_some();
        if !path.is_empty() && (subs.is_empty() || has_operands) {
            out.insert(path.to_owned());
        }
        for sub in subs {
            let child = if path.is_empty() {
                sub.get_name().to_owned()
            } else {
                format!("{path} {}", sub.get_name())
            };
            walk(sub, &child, out);
        }
    }

    let mut root = canvas_cli::dist::command();
    root.build();
    let mut out = BTreeSet::new();
    walk(&root, "", &mut out);
    out
}

/// Command paths named by the rows of the README `## Commands` table.
fn readme_commands() -> BTreeSet<String> {
    let table = README
        .split("\n## Commands\n")
        .nth(1)
        .expect("README has a `## Commands` section")
        .split("\n## ")
        .next()
        .expect("split always yields a first element");
    let mut rows = BTreeSet::new();
    for line in table.lines() {
        let line = line.trim();
        if !line.starts_with("| `canvas") {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split(" | ").collect();
        assert_eq!(cells.len(), 2, "row needs 2 cells: {line}");
        let command = cells[0].trim().trim_matches('`');
        let path = command
            .strip_prefix("canvas ")
            .unwrap_or_else(|| panic!("row command must start with `canvas `: {line}"));
        assert!(
            rows.insert(path.to_owned()),
            "duplicate README row for {path}"
        );
    }
    rows
}

#[test]
fn readme_command_table_matches_the_clap_command_list() {
    let clap: BTreeSet<String> = clap_commands();
    let readme: BTreeSet<String> = readme_commands();
    let missing: Vec<_> = clap.difference(&readme).collect();
    let extra: Vec<_> = readme.difference(&clap).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "README command table is out of date.\n  missing from README: {missing:?}\n  \
         not a canvas command: {extra:?}"
    );
}

#[test]
fn readme_completions_row_names_every_shell_the_binary_supports() {
    let row = README
        .split("| `canvas completions` |")
        .nth(1)
        .expect("README documents `canvas completions`")
        .lines()
        .next()
        .expect("the row has a first line");
    for shell in canvas_cli::dist::SHELLS {
        let name = shell.to_string();
        assert!(
            row.contains(&name),
            "README completions row omits {name}: {row}"
        );
    }
}
