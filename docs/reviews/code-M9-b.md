# Code review :  M9-b, `canvas mcp` is one tool (lane `lane/w1`)

## Verdict

**MERGE.** The collapse itself is right and is right all the way down: a real
`canvas mcp` session serves exactly one tool named `getclitools`, every one of
the 43 names the server ever served is `METHOD_NOT_FOUND`, `resources/read`
and `subscriptions/listen` are unroutable, `resources/list` is empty, and the
answer is the real command reference :  75 commands, built from the same clap
tree the binary parses with and the same registry `canvas schema` prints, with
no network request made to build it. Nothing was quietly deleted: as delivered, the
CLI's whole `--help` tree and every pre-existing `canvas schema` page are
**byte-identical** to `main`. One of my own fixes moves two `description`
strings; it is declared under "What this review changed outside the MCP
surface" below.

Seven defects were found and fixed in six `review(M9-b):` commits. One is
serious and is the package's own premise: the reference told an agent how to
spell the commands, and the spelling it gave was wrong for every command with
a required argument group, a required option, or an optional subcommand :  so a
model that did what the tool exists for earned exit 2 on `canvas submit`,
`canvas inbox send`, `canvas inbox reply`, `canvas discussion reply`,
`canvas download` and `canvas note`.

Nothing is pushed and nothing is merged into `main`. Three items are left for
Rolf, all of them already named honestly by the package.

## Gates

`CARGO_TARGET_DIR=<checkout>`.

| Gate | As delivered (`5327363`) | After the fixes (`5eaad76`) |
|---|---|---|
| `cargo fmt --all --check` | clean | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean, no new `#[allow]` | clean, no new `#[allow]` |
| `cargo nextest run --all-features --no-fail-fast` | 891 run, 880 passed, **11 failed** | **894 run, 883 passed, 11 failed** |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok | same |
| `cargo +1.88 check --workspace --all-targets` | clean | clean |
| `cargo xtask bench --runs 3 --mcp` | every §13 target met | every §13 target met |
| `cd extension && npm test` | not re-run (untouched) | 52 passed, 0 failed |

`cargo nextest run` without `--no-fail-fast` stops at the first failure; the
counts above are the full run.

**The 11 failures are pre-existing and environmental, verified rather than
taken on trust.** `main` was exported to a clean tree and five of the eleven :
`todo_is_stub`, `m1b_commands_exit_auth_without_identity`,
`command_choices_accept_documented_forms`, `grades_without_an_identity_is_exit_3`,
`files_auth_without_identity_is_exit_3` :  were run there under the same target
dir. All five fail identically on `main`. This machine holds a real stored
identity, so every test asserting "no identity ⇒ exit 3" gets exit 13. None of
the eleven touches MCP, the skill, or the registry.

**Bench, re-measured here, not copied from the report.** `tools/list` is one
tool, **656 bytes / ~164 tokens** for the definition; the whole JSON-RPC result
is **729 bytes**, which `tests/mcp.rs` prints when run with `--nocapture`, and
705 bytes over the `2025-11-25` adapter, which carries no `_meta`. Every §13
target met; warm `getclitools` round trip p50 4.3 ms against a 100 ms target.
The answer was 32 420 bytes as delivered and is **33 144 bytes / ~8 286 tokens**
after the fixes below made the usage lines complete. 75 commands, unchanged.

**Test-count accounting, checked name by name and not only in total.** Test
functions were counted per file at the M9 tip (`1cd780d`) and at `5327363`:
`mcp::resources` −7, `mcp::subscribe` −6, `mcp::result` −6, `tests/mcp.rs` −9,
`mcp::catalog` −3, `mcp::server` −1, `tests/skill.rs` −2, `mcp::reference` +4,
`output::json_schema` +1, `tests/schema_cmd.rs` +1, `tests/bridge.rs` 0,
`xtask::bench_mcp` 0. −34 and +6 is −28, and 919 − 28 = 891, which is what the
runner reports. The report's §8.4 table is exact.

## What was verified on the wire and against `main`

- **One tool, live.** A real `canvas mcp` process was driven with hand-written
  JSON-RPC over a pipe. `tools/list` returns one tool, `getclitools`, with
  `readOnlyHint`/`idempotentHint` true, `destructiveHint`/`openWorldHint`
  false, an empty `inputSchema` with `additionalProperties: false`, and **no**
  `outputSchema`. `resources/list` is `[]`; `resources/read`,
  `subscriptions/listen` and `todo.list` are all `-32601`.
- **The answer is real, not a stub.** 33 144 bytes, 75 `### canvas …`
  sections. The writes are there with their operands and flags
  (`canvas discussion reply`, `canvas submit`, `canvas inbox send|reply`), and
  so are the companion commands (`canvas bridge install|host|status|detach`,
  `canvas here`, `canvas note`, `canvas open --follow`). All **61** distinct
  `canvas schema "<name>"` names the reference cites were run against the
  binary: every one resolves, exit 0.
- **The CLI did not move.** Every `--help` page of the whole command tree was
  dumped from a `main` build and from this branch and diffed: **1 547 lines,
  identical**, before and after this review's fixes. Every `canvas schema
  <name>` page that `main` knows was diffed the same way and is
  **byte-identical as delivered**. `canvas schema --list` gained exactly the
  nine alias rows and nothing else. The M9-b diff touches `commands/` only to
  delete `here::foreign_consumer`.
- **`foreign_consumer` was genuinely dead.** `git grep` at the M9 tip shows one
  caller, `mcp/resources.rs:184`. Removing it changes no command.
- **§19 item 49 point 1 was right about the resource.** At the M9 tip the only
  senders of `Op::Attach` were inside `#[cfg(test)]` modules, so nothing in the
  shipped binary ever attached an MCP consumer; and `resources.rs` answered a
  foreign handle through `here::foreign_consumer`, which refuses
  `NotAttached` unconditionally without asking the broker. The template really
  was permanently unreachable before the deletion. This is a cleanup, not a
  regression.
- **`result.rs` and `subscribe.rs` took nothing load-bearing.** `mcp::result`'s
  only callers at the M9 tip were `resources.rs` and the tool-result arm of
  `server.rs`, both gone; `output::document_for_schema`'s only caller was
  `catalog::output_schema`, gone. §22 is untouched and `canvas watch --jsonl`
  is unchanged.
- **The skill's own command lines run.** All **71** `canvas …` lines in the
  shipped skill's ```sh``` blocks were fed to the real binary under a
  non-existent profile: **none** is a usage error. Every one reaches exit 3 or
  better.
- **Nothing leaks.** The reference generated from a session bound to the real
  stored identity contains no origin, user id, identity key or token.

## Defects found and fixed

| # | Sev | Where | What was wrong | What I changed | Commit |
|---|---|---|---|---|---|
| 1 | **High** | `mcp/reference.rs:202` (`usage_tail`) | The usage line was hand-rolled. It dropped **every required argument group** and **every required option**, and appended `<SUBCOMMAND>` whenever a command had subcommands at all, whether or not clap requires one. So the reference said `canvas submit [OPTIONS] <TARGET> [ASSIGNMENT]` (real: `<--file <FILES>\|--text <TEXT>\|--html <HTML>\|--url <URL>>` is required), `canvas inbox send [OPTIONS]` (real: `--to <TO>` and one of `--text`/`--text-file`), `canvas note [OPTIONS]` (real: `--text <TEXT>`), `canvas download [OPTIONS] [COURSE]` (real: `<COURSE\|--all-courses>`), and `canvas inbox [OPTIONS] <SUBCOMMAND>` where `canvas inbox --json` is the read the shipped workflow runs. A model following the reference earns exit 2 on the five writes the package exists to hand over. | `usage_lines` clones the node, names it after its full path, drops `--help`, and asks clap to `render_usage()` :  the same tree the binary parses with. All 75 usage lines now match their own `--help` byte for byte, alternative forms included. | `37d3f99` |
| 2 | Med | `mcp/reference.rs:218` (`operand`) | A positional standing for several values spelled only the first, so `canvas submission` read `<COURSE...>` :  a repeated course :  instead of `<COURSE> [ASSIGNMENT]...`, and contradicted the usage line above it. | All value names are spelled; the first `min_values` are required, the rest optional. | `37d3f99` |
| 3 | **High** | `mcp/reference.rs:37,95` (`PREAMBLE`) | "Add `--json` to any command below and it prints one envelope", and a global-flag block listing `--json` as working everywhere. Seven of the 75 refuse it with exit 2: `notify`, `completions`, `schema`, `config edit`, `bridge host`, `mcp` print raw output, and `canvas watch` takes `--jsonl`. Each already said the right thing in its own `Returns` line, so the preamble contradicted the body of the same document. | The preamble sends the reader to the `Returns` line for the flag; the global block names the one exception. | `5eaad76` |
| 4 | Med | `docs/companion.md:96` | The replacement two-consumer check :  "run `canvas here` from two terminals under different profiles", expecting "each reads only after its own attach" :  does not test the rule. A profile is an identity and an endpoint is `Endpoint::for_identity`, so two profiles are two brokers and two attachments; and `canvas here` from the CLI passes `consumer: None`, which takes the sole attachment rather than naming a consumer. It would pass whether the rule held or not. | The row says there is nothing to run and why, and points at `canvas_core::bridge::state::tests::only_an_opted_in_consumer_reads_the_bundle`, which still holds the rule. | `5d7add9` |
| 5 | Med | `xtask/src/bench.rs:1153` → `docs/bench.md:44` | The generated doc headed the 656-byte figure "the bytes the whole tool list puts on the wire". The wire result is 729; 656 is the definitions alone. SPEC §21.2 already distinguishes the two. | The sentence names which figure it is, why it is the comparable one, and where the wire number is measured. The table is unchanged, because the 167 955 and 344 878 it is compared with were measured the same way. | `898ac82` |
| 6 | Low | `mcp/reference.rs:280` (`help_of`) | An appended "One of: …" ran into a clap help string that carries no stop: "Which Chromium-family browser to install for One of: chrome, chromium, edge." Three arguments. | `end_sentence` closes the help text first. | `37d3f99` |
| 7 | Low | `commands/open.rs:217`, `output/registry.rs:769,2294,2306` | Doc comments naming `context.follow` and `submission.execute` in the present tense as live callers. Both tools left in M9. | Past-tensed, and the live caller named (`canvas open --follow`; nothing reaches the replay path, which is why `replayed` is always `false`). The equivalent comments in `canvas-core` describe the broker's own IPC vocabulary, which the extension still speaks, and are left alone as out of scope. | `ea8109d` |

`ce0c532` carries no fix of its own: it re-measures the answer and updates the
three places SPEC quotes its size, after #1 and #2 made the usage lines
complete.

**Three tests were added**, all in `mcp::reference`:
`every_usage_line_is_the_one_the_command_itself_prints` walks the whole clap
tree and asserts the reference carries each command's own rendered usage;
`the_writes_say_what_they_cannot_run_without` pins the six spellings by hand,
so the check survives a change in clap's rendering; and
`the_preamble_sends_the_reader_to_the_returns_line_for_the_flag` pins the
flag rule. Defect #1 is exactly the class of bug that a "does it mention the
command?" test cannot see, and the package had only that.

## The deviations the report names, checked

1. **`output/registry.rs` and `output/json_schema.rs` were touched.** Accepted.
   The brief asked for `canvas schema`'s coverage to be extended rather than a
   second description format invented, and the alias table is the least
   invasive form: no `SchemaEntry`, no fixture, no snapshot. Verified byte for
   byte :  every page `main` can print is unchanged, and `--list` gains exactly
   nine rows. `alias_command` returns `None` for any name a real entry owns, so
   an alias can never rewrite an existing page's `command` field. The nine
   aliases were checked against the commands: `auth token` without `--reveal`
   calls `status()` and really does print `auth_status@1`.
2. **`xtask/src/bench_mcp.rs` and `bench.rs` were rewritten.** Accepted; the
   `--mcp` bench is a gate and the old harness called `todo.list`.
3. **`commands/here.rs` lost `foreign_consumer`.** Accepted, verified dead.
4. **`docs/companion.md` lost a row's wording.** The change was right to make
   and wrong in what it replaced it with; see defect #4.
5. **`ToolSpec` kept, trimmed to four fields.** Accepted.
6. **`session-ses_f729.md`.** Untracked, left on disk, untouched by this
   review. The working tree is otherwise clean.

## What this review changed outside the MCP surface

One thing, and it is declared here rather than folded into the table above.

`schemars` reads Rust doc comments and publishes them as `description` strings,
so the stale comments of defect #7 were not only comments: two of them were in
the **published schema**. `canvas schema here` told a reader that
`browser.follow` records "the last `context.follow` this consumer asked for",
naming a tool that has not existed since M9, and `canvas schema submit`
described `replayed` through a live `submission.execute` replay path that is
also gone.

Fixing the comments therefore moved two pages. The change was measured: `here`
and `submit` differ in **four lines each, all of them `description` strings**,
and no other page moves. No type, no `required` list, no `enum`, no
`additionalProperties`, no property name, and no fixture changed :  a validator
sees the same documents. Every other page `main` knows is still byte-identical.

I kept it. A published `description` that names a removed tool as a live caller
is wrong in the contract, not only in the source, and this package is the one
that removed the tool. Correcting prose in a page is the smallest possible
change that makes the contract true, and `canvas-cli` is pre-1.0 with the
schema id unchanged at `@1` in both cases.

## Needs a decision

All three are Rolf's, and all three are already recorded honestly by the
package rather than used to cover a defect. I confirmed each against the code.

1. **`open::follow`'s stale-generation guard is correct and dead** (§19 item 49
   point 2). Verified: `main.rs:255` is the only caller and passes `None` for
   both `consumer` and `generation`. Either add `--generation` to
   `canvas open --follow`, or accept that a follow from a terminal is the
   person's own act. Nothing is broken either way.
2. **§19 item 17's replay rule has no surface** (§19 item 49 point 3).
   Verified: nothing reaches the path, `submit@1.replayed` is always `false`,
   and `canvas submit` refuses an already-executed plan with exit 8 as it
   always did. The field is additive and the rule is still the right one if an
   approved-plan surface returns.
3. **No third-party host has been run against the one-tool build.** Nobody has
   watched Claude Code, Cursor or Codex load this build, call `getclitools`,
   and go on to run a `canvas` command :  and that is the whole premise of the
   design. `docs/agent-hosts.md` says so in three places and marks both
   third-party rows "(then)". This review did not change that: the only clients
   exercised here are the project's own. It is the one thing left that evidence
   could still overturn, and it needs a person at a host, not another test.

The cost recorded in item 50 :  a host with no execution capability can now only
show the student a command line :  is Rolf's accepted trade, and is not
re-argued here.

## Notes, not defects

- `canvas auth token` now answers `canvas schema` through the `auth_status@1`
  alias, and the reference's `Returns` line says "with `--json`". That is true
  of the bare command; `auth token --reveal` refuses `--json` with exit 2
  (SPEC §7). The registry comment records the nuance and the `--reveal` help
  says "Print the raw token", so a reader is not misled about what the command
  does. Left as is rather than special-cased.
- The reference documents `canvas auth token --reveal` to an agent that has a
  shell. It grants nothing: `canvas --help` already lists it, and the MCP
  server itself still cannot reveal a credential :  it has no tool that runs
  anything. §15 containment is unchanged.
- `usage_lines` clones one `clap::Command` per node. The warm round trip is
  4.3 ms against a 100 ms target, so the cost is not visible.
