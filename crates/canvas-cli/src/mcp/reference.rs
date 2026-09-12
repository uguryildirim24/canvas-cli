//! The `canvas` command reference `getclitools` returns (§21.2).
//!
//! `canvas mcp` serves one tool, and this module is its whole answer. Nothing
//! here is written by hand twice. The commands, their operands, and their
//! flags come from the clap tree the binary itself parses
//! ([`canvas_cli::dist::command`]), which also backs the man pages and the
//! completion scripts. What each command returns comes from the registry
//! `canvas schema` prints, through [`crate::output::document_for_command`].
//! A command therefore cannot be described here differently from the way it
//! behaves, and `canvas schema` and `getclitools` describe the same CLI the
//! same way.

use std::fmt::Write as _;

use clap::{Arg, ArgAction, Command};
use serde_json::Value;

/// What the caller is told before the command list.
///
/// It says the one thing this surface exists to say: the tool is a map, not a
/// door. Everything after this is run with the caller's own shell.
const PREAMBLE: &str = "\
# The `canvas` command line

This is the whole surface. `canvas mcp` serves exactly one tool — \
`getclitools`, which you have just called — and its only job is to hand you \
this reference. There is no second tool, no resource, and no subscription: \
from here on, run `canvas <command> ...` yourself with whatever shell or \
execution capability you have. Every read and every write is a command.

`canvas-cli` is a **student** tool bound to one identity. It has no teacher, \
TA, or admin features. It cannot reveal a credential, change an identity, run \
arbitrary HTTP or shell, or clear the cache on your behalf.

## Reading an answer

Every command whose `Returns` line names a schema prints one envelope \
(SPEC §7) when you add the flag that line names — `--json` for almost all of \
them, `--jsonl` for `canvas watch`:

```json
{
  \"schema\": \"canvas-cli/todo@1\",
  \"generated_at\": \"...\",
  \"profile\": \"default\",
  \"identity\": { \"origin\": \"...\", \"user_id\": \"...\", \"key\": \"...\" },
  \"freshness\": [{ \"dataset\": \"planner\", \"source\": \"cache\", \"fetched_at\": \"...\", \"complete\": true, \"stale\": false, \"count\": 12 }],
  \"requests\": { \"api\": 0, \"storage\": 3, \"cost\": null },
  \"partial\": [],
  \"warnings\": [],
  \"outcome\": \"ok\",
  \"exit\": 0,
  \"result\": {}
}
```

Read it in this order: `outcome`, then `exit`, then `partial` and \
`warnings`, then `freshness`, then `result`. A refusal is an answer, not a \
crash. `null` means unknown or not applicable; it never means zero. Grades \
are what Canvas reports — nothing here computes one.

Never invent a field. `canvas schema <command>` prints the full JSON Schema \
of any command's envelope and result, and `canvas schema --list` prints every \
registered schema. The `Returns` line of each command below names the schema \
and the command name that resolves it.

## Exit codes

| Exit | Meaning | What to do |
|---|---|---|
| 0 | Success | Use the result. Report `freshness` if it is stale. |
| 1 | Generic failure | Report it. Do not retry the same call. |
| 2 | Usage | The arguments were wrong. Read the message, fix them, run it once more. |
| 3 | Auth | No token, an expired token, or no identity. Ask the user to run `canvas auth login`. Never retry. |
| 4 | Network | DNS, TLS, or a timeout. Retry once, then report it and offer cached data. |
| 5 | Rate limited | Canvas is throttling. Stop calling. Tell the user to wait. |
| 6 | Resolution | Zero or many matches. Show the candidates and ask which one. |
| 7 | Offline miss | The cache has no coverage and the session is offline. Run `canvas sync`, or say you are offline. |
| 8 | Refused | Not allowed as asked. Read `result` for the reason. Never work around it. |
| 9 | Submission recovery | A submit did not finish. Reconcile it. Never submit again first. |
| 10 | Verification mismatch | Canvas and the local receipt differ. Show both. Overwrite nothing. |
| 11 | Cancelled | The person said no. Stop. |
| 12 | Partial | Some of the answer is missing. Read `partial`, name what failed, use the rest. |
| 13 | Local | A database, lock, or identity problem on this machine. Run `canvas doctor`. |

## Writes

Submitting work, replying to a discussion, writing to the Canvas inbox, \
downloading files, and retiring a receipt are commands, not tools. Each one \
prints what it is about to do and asks for a confirmation at the terminal, \
and that confirmation belongs to the person. **Never pass `--yes`.** It \
exists for a person who means it; an agent that passes it has taken the \
decision away from them.

## Global flags

These work on every command below, and are not repeated for each one:

    --json                Machine-readable output: one SPEC §7 envelope.
    --profile <NAME>      Which stored identity to use.
    --offline             Never touch the network; answer from the cache.
    --fresh               Ignore the cache TTLs.
    --color <WHEN>        auto, always, or never.
    -q, --quiet           No progress, no info logs.
    -v, --verbose         Debug logs on stderr.

`--json` is the one exception. A command whose `Returns` line says \
`raw output` has no envelope to print and refuses `--json` with exit 2, and \
`canvas watch` takes `--jsonl` instead. Read the `Returns` line before you \
add the flag.

## Commands
";

/// The end of the answer: the registry listing `canvas schema --list` prints.
const LISTING_HEADING: &str = "\
## Schema names

Every name in the first column resolves with `canvas schema <name>`, which \
prints the full JSON Schema of that document. `command` is a name a person \
can run; `document` is a payload no command prints on its own. This is the \
listing `canvas schema --list` writes, unchanged.

```
name\tschema\tkind
";

/// The whole `canvas` command reference, as Markdown.
#[must_use]
pub fn reference() -> String {
    let root = canvas_cli::dist::command();
    let mut out = String::with_capacity(64 * 1024);
    out.push_str(PREAMBLE);
    for sub in root.get_subcommands() {
        walk(sub, &format!("canvas {}", sub.get_name()), &mut out);
    }
    out.push('\n');
    out.push_str(LISTING_HEADING);
    out.push_str(&crate::output::schema_list());
    out.push_str("```\n");
    out
}

/// Describe one command, then each of its subcommands.
fn walk(command: &Command, path: &str, out: &mut String) {
    if command.is_hide_set() || command.get_name() == "help" {
        return;
    }
    describe(command, path, out);
    for sub in command.get_subcommands() {
        walk(sub, &format!("{path} {}", sub.get_name()), out);
    }
}

/// One command: what it is for, how it is spelled, and what it returns.
fn describe(command: &Command, path: &str, out: &mut String) {
    let _ = writeln!(out, "\n### {path}\n");
    if let Some(about) = command.get_about() {
        let _ = writeln!(out, "{about}\n");
    }
    for line in usage_lines(command, path) {
        let _ = writeln!(out, "    {line}");
    }
    out.push('\n');

    let operands: Vec<&Arg> = visible_args(command)
        .filter(|arg| arg.is_positional())
        .collect();
    if !operands.is_empty() {
        let _ = writeln!(out, "Operands:");
        for arg in operands {
            let _ = writeln!(out, "  {:<22} {}", operand(arg), help_of(arg));
        }
    }
    let options: Vec<&Arg> = visible_args(command)
        .filter(|arg| !arg.is_positional())
        .collect();
    if !options.is_empty() {
        let _ = writeln!(out, "Options:");
        for arg in options {
            let _ = writeln!(out, "  {:<22} {}", option(arg), help_of(arg));
        }
    }
    let subcommands: Vec<&str> = command
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set() && sub.get_name() != "help")
        .map(Command::get_name)
        .collect();
    if !subcommands.is_empty() {
        let _ = writeln!(out, "Subcommands: {}", subcommands.join(", "));
    }
    // A command that is only a group of subcommands runs nothing of its own,
    // so it returns nothing of its own either.
    if !command.is_subcommand_required_set() {
        let _ = writeln!(out, "{}", returns(path));
    }
}

/// The arguments a caller can actually pass here.
///
/// Global flags are listed once in the preamble, and `--help`/`--version`
/// need no description, so neither is repeated for sixty commands.
fn visible_args(command: &Command) -> impl Iterator<Item = &Arg> {
    command.get_arguments().filter(|arg| {
        !arg.is_hide_set()
            && !arg.is_global_set()
            && !matches!(arg.get_id().as_str(), "help" | "version")
    })
}

/// The usage lines of one command, as clap itself spells them.
///
/// clap renders them from the same tree it parses with, so the reference
/// cannot claim a spelling the binary would reject: a required option
/// (`--to <TO>`), a required one-of group (`<--text <TEXT>|--text-file
/// <TEXT_FILE>>`), an optional subcommand, and a command with two alternative
/// forms all come out the way `canvas <command> --help` prints them. Rolling
/// this by hand is what made `canvas inbox` read as `<SUBCOMMAND>`-only and
/// lost every required group.
///
/// The clone carries `path` as its binary name so the lines read
/// `canvas inbox send`, and drops `--help`, which is described once in the
/// preamble and would otherwise put `[OPTIONS]` on commands that take none.
fn usage_lines(command: &Command, path: &str) -> Vec<String> {
    let mut spelled = command
        .clone()
        .bin_name(path.to_owned())
        .disable_help_flag(true);
    spelled
        .render_usage()
        .to_string()
        .lines()
        .map(|line| {
            line.trim_start()
                .trim_start_matches("Usage:")
                .trim()
                .to_owned()
        })
        .filter(|line| !line.is_empty())
        .collect()
}

/// A positional argument: `<COURSE>`, `[COURSE]`, or `<COURSE> [ASSIGNMENT]...`.
///
/// One positional can stand for several values — `canvas submission` takes a
/// course and then an assignment through a single `1..=2` argument — so every
/// value name is spelled, not just the first. Only the first `min_values` of
/// them are required, which is what makes the second one `[ASSIGNMENT]`.
fn operand(arg: &Arg) -> String {
    let names: Vec<String> = arg
        .get_value_names()
        .map(|names| names.iter().map(ToString::to_string).collect())
        .filter(|names: &Vec<String>| !names.is_empty())
        .unwrap_or_else(|| vec![arg.get_id().as_str().to_ascii_uppercase()]);
    let required = arg.get_num_args().map_or(1, |range| range.min_values());
    let mut spelling = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            if arg.is_required_set() && index < required {
                format!("<{name}>")
            } else {
                format!("[{name}]")
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if takes_many(arg) {
        spelling.push_str("...");
    }
    spelling
}

/// A flag or an option, as it is typed.
fn option(arg: &Arg) -> String {
    let mut spelling = String::new();
    if let Some(short) = arg.get_short() {
        let _ = write!(spelling, "-{short}, ");
    }
    if let Some(long) = arg.get_long() {
        let _ = write!(spelling, "--{long}");
    }
    if takes_a_value(arg) {
        let value = arg
            .get_value_names()
            .and_then(|names| names.first().map(ToString::to_string))
            .unwrap_or_else(|| arg.get_id().as_str().to_ascii_uppercase());
        let _ = write!(spelling, " <{value}>");
    }
    spelling
}

fn takes_a_value(arg: &Arg) -> bool {
    matches!(arg.get_action(), ArgAction::Set | ArgAction::Append)
}

fn takes_many(arg: &Arg) -> bool {
    matches!(arg.get_action(), ArgAction::Append)
        || arg
            .get_num_args()
            .is_some_and(|range| range.max_values() > 1)
}

/// One argument's help, with the values it accepts and its default.
fn help_of(arg: &Arg) -> String {
    let mut text = arg
        .get_help()
        .map(ToString::to_string)
        .unwrap_or_default()
        .replace('\n', " ");
    if !takes_a_value(arg) {
        return text.trim().to_owned();
    }
    let values: Vec<String> = arg
        .get_possible_values()
        .iter()
        .filter(|value| !value.is_hide_set())
        .map(|value| value.get_name().to_owned())
        .collect();
    if !values.is_empty() {
        end_sentence(&mut text);
        let _ = write!(text, "One of: {}.", values.join(", "));
    }
    let defaults: Vec<String> = arg
        .get_default_values()
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    if !defaults.is_empty() {
        end_sentence(&mut text);
        let _ = write!(text, "Default: {}.", defaults.join(", "));
    }
    text.trim().to_owned()
}

/// Close whatever `text` holds so the next sentence starts as one.
///
/// A clap help string carries no trailing stop, so appending straight to it
/// read as "Which Chromium-family browser to install for One of: chrome".
fn end_sentence(text: &mut String) {
    let trimmed = text.trim_end();
    text.truncate(trimmed.len());
    if text.is_empty() {
        return;
    }
    if !text.ends_with(['.', '!', '?', ':']) {
        text.push('.');
    }
    text.push(' ');
}

/// What this command prints, from the same registry `canvas schema` reads.
///
/// A command with no registered document prints no §7 envelope at all: those
/// are the raw-output commands of §7, and saying so is the answer.
fn returns(path: &str) -> String {
    let name = path.strip_prefix("canvas ").unwrap_or(path);
    let Some(document) = crate::output::document_for_command(name) else {
        return "Returns: raw output. This command writes no §7 envelope, so it has \
                no schema (SPEC §7)."
            .to_owned();
    };
    let schema = document["schema"].as_str().unwrap_or_default();
    let flag = document["output"]["flag"].as_str().unwrap_or("--json");
    let fields = result_fields(&document);
    let mut line = format!("Returns: `{schema}` with `{flag}`");
    if !fields.is_empty() {
        let _ = write!(line, ", `result` fields: {}", fields.join(", "));
    }
    let _ = write!(line, ". Full schema: `canvas schema \"{name}\"`.");
    line
}

/// The top-level field names of a command's `result` payload.
fn result_fields(document: &Value) -> Vec<String> {
    let payload = if document.get("line").is_some() {
        &document["line"]
    } else {
        &document["result"]
    };
    payload["properties"]
        .as_object()
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every command of the clap tree is in the reference, under its own
    /// full path. A command a host cannot see is a command it cannot run.
    #[test]
    fn every_command_of_the_clap_tree_is_described() {
        let text = reference();
        let root = canvas_cli::dist::command();
        let mut checked = 0;
        let mut stack: Vec<(String, Command)> = root
            .get_subcommands()
            .map(|sub| (format!("canvas {}", sub.get_name()), sub.clone()))
            .collect();
        while let Some((path, command)) = stack.pop() {
            if command.is_hide_set() || command.get_name() == "help" {
                continue;
            }
            assert!(
                text.contains(&format!("\n### {path}\n")),
                "{path} is missing"
            );
            checked += 1;
            for sub in command.get_subcommands() {
                stack.push((format!("{path} {}", sub.get_name()), sub.clone()));
            }
        }
        assert!(checked > 40, "only {checked} commands were described");
    }

    /// Every command's usage line is clap's own, so a spelling the reference
    /// prints is a spelling the binary accepts.
    ///
    /// This is the test the first cut did not have. A hand-rolled tail lost
    /// every required argument group and every required option, and told an
    /// agent that `canvas inbox` needs a subcommand when the bare form is the
    /// read, so a model that followed the reference earned exit 2.
    #[test]
    fn every_usage_line_is_the_one_the_command_itself_prints() {
        let text = reference();
        let root = canvas_cli::dist::command();
        let mut stack: Vec<(String, Command)> = root
            .get_subcommands()
            .map(|sub| (format!("canvas {}", sub.get_name()), sub.clone()))
            .collect();
        let mut checked = 0;
        while let Some((path, command)) = stack.pop() {
            if command.is_hide_set() || command.get_name() == "help" {
                continue;
            }
            for line in usage_lines(&command, &path) {
                assert!(
                    text.contains(&format!("\n    {line}\n")),
                    "{path} is not spelled the way it parses: {line}"
                );
                checked += 1;
            }
            for sub in command.get_subcommands() {
                stack.push((format!("{path} {}", sub.get_name()), sub.clone()));
            }
        }
        assert!(checked > 40, "only {checked} usage lines were checked");
    }

    /// A required option and a required one-of group are on the usage line.
    ///
    /// These are the three writes and the two commands an agent cannot run at
    /// all without them, spelled out here so the check does not depend on
    /// clap's rendering staying identical.
    #[test]
    fn the_writes_say_what_they_cannot_run_without() {
        let text = reference();
        for required in [
            "canvas submit [OPTIONS] <--file <FILES>|--text <TEXT>|--html <HTML>|--url <URL>> \
             <TARGET> [ASSIGNMENT]",
            "canvas inbox send [OPTIONS] --to <TO> <--text <TEXT>|--text-file <TEXT_FILE>>",
            "canvas inbox reply [OPTIONS] <--text <TEXT>|--text-file <TEXT_FILE>> \
             <CONVERSATION_ID>",
            "canvas discussion reply [OPTIONS] <--text <TEXT>|--text-file <TEXT_FILE>> \
             <COURSE> <DISCUSSION>",
            "canvas quiz submit [OPTIONS] --answers <ANSWERS> <COURSE> <QUIZ>",
            "canvas download [OPTIONS] <COURSE|--all-courses>",
            "canvas note [OPTIONS] --text <TEXT>",
        ] {
            assert!(
                text.contains(required),
                "the reference never spells: {required}"
            );
        }
        // A command whose subcommand is optional says so, and its bare form
        // is the one the skill's own workflows run.
        assert!(text.contains("canvas inbox [OPTIONS] [COMMAND]"), "{text}");
        assert!(text.contains("canvas open [OPTIONS] <TARGET>"), "{text}");
        // One positional can stand for two values, and both are named.
        assert!(text.contains("<COURSE> [ASSIGNMENT]..."), "{text}");
    }

    /// The reference names the writes as commands, with their flags.
    #[test]
    fn the_write_commands_carry_their_operands_and_what_they_return() {
        let text = reference();
        for required in [
            "### canvas submit",
            "### canvas discussion reply",
            "### canvas inbox send",
            "### canvas inbox reply",
            "### canvas quiz submit",
            "### canvas operation status",
            "### canvas download",
            "canvas-cli/operation@1",
            "canvas-cli/submit@1",
        ] {
            assert!(
                text.contains(required),
                "the reference never says {required}"
            );
        }
    }

    /// The preamble does not promise `--json` where the binary refuses it.
    ///
    /// `canvas notify`, `canvas completions`, `canvas schema`,
    /// `canvas config edit`, `canvas bridge host` and `canvas mcp` reject
    /// `--json` with exit 2 (SPEC §7), and `canvas watch` takes `--jsonl`.
    /// A blanket "add `--json` to any command below" earned a usage error on
    /// seven of the seventy-five.
    #[test]
    fn the_preamble_sends_the_reader_to_the_returns_line_for_the_flag() {
        let text = reference();
        assert!(
            !text.contains("Add `--json` to any command below"),
            "the preamble promises `--json` everywhere"
        );
        assert!(text.contains("`--jsonl` for `canvas watch`"), "{text}");
        assert!(
            text.contains("refuses `--json` with exit 2"),
            "the global flag block does not name the exception"
        );
    }

    /// A raw-output command says so rather than promising an envelope.
    #[test]
    fn a_raw_output_command_promises_no_envelope() {
        let text = reference();
        let notify = text
            .split("### canvas notify")
            .nth(1)
            .expect("notify is described");
        let block = notify.split("\n### ").next().unwrap_or_default();
        assert!(
            block.contains("Returns: raw output"),
            "notify claims an envelope: {block}"
        );
    }

    /// The listing `canvas schema --list` prints closes the answer, so the
    /// names a caller can resolve are the names the CLI resolves.
    #[test]
    fn the_schema_listing_is_the_one_the_cli_prints() {
        let text = reference();
        assert!(text.contains(&crate::output::schema_list()));
    }
}
