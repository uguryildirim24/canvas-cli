# Code review + fix — M9 on branch lane/w1 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M9
on branch `lane/w1` (worktree `/Users/rolfie/projects/canvas-cli`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /Users/rolfie/projects/canvas-cli`. Read the package brief `tasks/m9-mcp-reads-only.md` and the SPEC sections it
   cites. Then read `git log main..HEAD --stat` and the full diff. Base is `9394895`; 4 commits: `feat(mcp)!: narrow the tool catalog to the 22 read tools`, `test(mcp): pin the read-only catalog and drop the approval round trip`, `docs(skill): route every write through the CLI, not through MCP`, `docs(spec): §19 item 48 — the MCP catalog is reads only, and what that cost`. Contract: the owner's own words, quoted verbatim in §19 item 48: "mcp should only list get tools thats it", confirmed scope: keep exactly the 22 named tools (`courses.list, course.get, todo.list, assignments.list, assignment.get, grades.get, files.list, modules.list, pages.list, page.get, syllabus.get, announcements.list, announcement.get, discussions.list, discussion.get, inbox.list, inbox.get, inbox.unread_count, calendar.list, submission.get, receipts.list, receipts.show`), remove the other 21 (`sync.run, download.plan, download.run, submission.prepare, submission.execute, submission.reconcile, discussion.reply.prepare, discussion.reply.execute, inbox.send.prepare, inbox.send.execute, inbox.reply.prepare, inbox.reply.execute, operation.status, operation.reconcile, receipts.acknowledge, open.url, context.attach, context.here, context.detach, context.note, context.follow`), touch nothing under `extension/`, `canvas-core`, or any CLI command's own behavior. Package brief: `tasks/m9-mcp-reads-only.md`; SPEC §19 items 48-49, §21. Attack it as an attacker and as a completeness check, both: (1) Grep the whole tree for every one of the 21 removed tool names as a literal string — in `mcp/catalog.rs`, `mcp/server.rs`, any other MCP module, `tests/mcp.rs`, `skill/canvas-cli/SKILL.md` and every skill workflow file, `docs/agent-hosts.md`, `docs/SPEC.md`, `docs/writes-v2.md`, `docs/bench.md`, the README — a stray survivor in a doc that still tells an agent to call a tool that no longer exists is a defect even though it cannot execute. (2) Confirm `tools/list` over a real `canvas mcp` session (or the equivalent test) returns exactly the 22 names, in the claimed catalog-size numbers (22 tools, the byte and token counts w1 measured with `cargo xtask bench --mcp`, not estimated). (3) Confirm every one of the 22 kept tools truly has zero side effect: read the handler each dispatches to, not just its `effect:` annotation (w1's brief said an annotation that disagreed with reality should be fixed, not trusted — verify that was actually done, not just claimed). (4) Confirm the CLI itself is completely unchanged: `canvas submit`, `discussion reply`, `inbox send|reply`, `operation status|reconcile`, `download`, `sync`, `bridge *`, `note`, `open --follow` still exist, still work, and their own tests (outside `tests/mcp.rs`) are untouched — diff `main...HEAD` for any file under `crates/canvas-cli/src/commands/`, `crates/canvas-core/`, or `extension/` and treat any hit there as a defect unless w1's report justifies it in writing. (5) Confirm the removed `input_required`/elicitation approval-round-trip code is actually gone, not merely made unreachable and left to bit-rot (dead code with `-D warnings` still green is the bar; if something survives that shouldn't, say why in your verdict rather than silently keeping it). (6) Confirm the new parity/invariant test in `tests/mcp.rs` would actually fail if someone added a write tool back to the catalog tomorrow — read it, don't take the description on faith; if it only pins today's 22 names and wouldn't catch a new write tool with a misleading `Read` annotation, that is a defect, fix it. (7) The report says 11 pre-existing test failures, verified identical against a detached baseline worktree at `9394895` — spot-check that comparison yourself (rerun a couple of the named tests against a fresh checkout of `9394895` in your own reviewer target dir) rather than trusting the report's diff. (8) Read the three "owner's call" items in the report (the `context/{consumer_handle}` resource, the now-unreachable stale-follow-generation guard, and §19 item 17's replay rule) against the actual code — confirm each is genuinely unreachable rather than silently broken, and that none of the three was a shortcut for something that should have been fixed instead of flagged. The worker's final report, for reference:

```
# M9 — Narrow the MCP tool catalog to reads only

Lane `w1`, worktree `/Users/rolfie/projects/canvas-cli`, branch `lane/w1`.
`CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/w1`.

**4 commits. Nothing pushed. Nothing merged into `main`.** `main` is still at
`9394895`.

```
9534ae5  docs(spec): §19 item 48 — the MCP catalog is reads only, and what that cost
bfafa27  docs(skill): route every write through the CLI, not through MCP
1038b64  test(mcp): pin the read-only catalog and drop the approval round trip
6c7115d  feat(mcp)!: narrow the tool catalog to the 22 read tools
```

Diffstat against `main`: 24 files, 789 insertions, 2884 deletions.

---

## 1. `crates/canvas-cli/src/mcp/catalog.rs`

The file went from 1846 lines to 1147, then to its final shape after the
dead-code pass.

### The 22 kept tools

In this order, unchanged:

```
courses.list, course.get, todo.list, assignments.list, assignment.get,
grades.get, files.list, modules.list, pages.list, page.get, syllabus.get,
announcements.list, announcement.get, discussions.list, discussion.get,
inbox.list, inbox.get, inbox.unread_count, calendar.list, submission.get,
receipts.list, receipts.show
```

### The 21 removed tools

`sync.run`, `download.plan`, `download.run`, `submission.prepare`,
`submission.execute`, `submission.reconcile`, `discussion.reply.prepare`,
`discussion.reply.execute`, `inbox.send.prepare`, `inbox.send.execute`,
`inbox.reply.prepare`, `inbox.reply.execute`, `operation.status`,
`operation.reconcile`, `receipts.acknowledge`, `open.url`, `context.attach`,
`context.here`, `context.detach`, `context.note`, `context.follow`.

For each: the `ToolSpec` entry, its dispatch arm, and its argument struct are
gone. The command core behind each arm was **not** touched — `submission::*`,
`operation::*`, `download::handle`, `sync::handle`, `note::handle`,
`open::handle`, `open::follow` all still exist and still serve the CLI.

A `tools/call` naming a removed tool is a JSON-RPC `METHOD_NOT_FOUND`
(`-32601`), never a silent success.

### 18 argument structs removed

`SyncRunArgs`, `DownloadArgs`, `SubmissionPrepareArgs`,
`SubmissionExecuteArgs`, `DiscussionReplyPrepareArgs`, `InboxSendPrepareArgs`,
`InboxReplyPrepareArgs`, `OperationExecuteArgs`, `OperationStatusArgs`,
`OperationReconcileArgs`, `ReconcileArgs`, `AcknowledgeArgs`, `OpenUrlArgs`,
`ContextAttachArgs`, `ContextHereArgs`, `ContextDetachArgs`, `ContextNoteArgs`,
`ContextFollowArgs`.

### `Effect` collapsed to one variant

```rust
pub enum Effect {
    /// An authorized read: nothing changes.
    Read,
}
```

This is the strongest available form of the invariant: a tool that is not a
read has **no spelling in `ToolSpec` at all**, so putting one back means
reopening the enum. It also had to happen for a second reason — with
`Organize`, `Retire` and `RemoteWrite` unconstructed, `clippy -D warnings`
fails, and the brief forbids papering over it with `#[allow(dead_code)]`.

The brief allowed "`effect: Effect::Read` (or equivalent)". Every `ToolSpec`
still literally carries `effect: Effect::Read`, so the wording is satisfied
directly, not by an equivalent.

### `dispatch` simplified

`Dispatched` was a two-variant enum (`Done(Handled)` /
`Approval(Box<submit::Pending>)`). The `Approval` arm is unreachable now, so
the enum is gone and the signature collapsed:

```rust
// before
pub async fn dispatch(globals, consumer, name, arguments) -> Result<Dispatched, String>
// after
pub async fn dispatch(globals, name, arguments) -> Result<Handled, String>
```

The `consumer` parameter went with it — it existed to stamp a plan and an
approval handle, and no tool creates either. `consumer_of()` is still used by
`read_resource` and `listen`, so consumer handles still work on the resource
and subscription surfaces.

That change turned 22 `.into()` calls into identity conversions, which clippy
flagged as `useless_conversion`; all 22 were removed.

### Also removed from `catalog.rs`

- `asks_for_approval(name)` — asked the catalog which tools end in `.execute`.
- `literal()`, `paths()`, `submit_args()` — argument shaping for the write
  tools, including the `-` refusal ("stdin carries the protocol here").

### Annotation semantics

`Effect::annotations` is unchanged in shape. Because `Effect` has one variant,
`read_only_hint` is now `Some(true)` for every tool, `destructive_hint` stays
`Some(false)`, and `idempotent_hint` / `open_world_hint` are still set per
tool. `open_world_hint` is `false` for `receipts.list` and `receipts.show`,
which read only the local record.

---

## 2. `crates/canvas-cli/src/mcp/server.rs`

The whole `input_required` / elicitation approval round trip was removed. It
existed only to service the four `*.execute` tools, and nothing in the
22-tool catalog can trigger it.

Removed: `APPROVAL_KEY`, `ApprovalState`, `CanvasServer::record_decision`,
`ask_approval`, `approval_message`, `declares_form_elicitation`, and the
imports `ElicitRequest`, `ElicitRequestParams`, `ElicitResult`,
`ElicitationAction`, `ElicitationSchema`, `InputRequest`, `InputRequests`,
`InputRequiredResult`, `InputResponses`, `ClientCapabilities`, plus
`commands::submit::{Pending, Refusal}` and `commands::submit`.

`call_tool` kept one guard rather than dropping the concept entirely. A
`requestState` is still reachable over the wire, and silently ignoring it
would let a replayed decision ride along with a read:

```rust
if request.request_state.is_some() {
    return Err(ErrorData::invalid_params(
        format!("{name} never asks for an approval: this catalog is read-only"),
        Some(serde_json::json!({ "tool": name })),
    ));
}
```

The server `INSTRUCTIONS` string was rewritten: it advertised
"Canvas LMS, read-first", told a model that `sync.run` refreshes the cache,
and said "A submission needs a recorded human approval before anything reaches
Canvas". It now says the surface is read-only, points at `canvas sync` for a
refresh, and lists what no tool does.

**Nothing else in `mcp/` was reachable-but-dead.** `resources.rs`,
`subscribe.rs` and `result.rs` are untouched; `consumer_of` remains live for
both.

---

## 3. The new parity / invariant test

### What was there

`mcp::every_tool_returns_the_envelope_the_cli_prints` compared each tool's
envelope with the equivalent `canvas` command's, and excluded names like this:

```rust
expected.retain(|name| !name.ends_with(".execute") && !name.starts_with("context."));
```

That exclusion list is now almost the whole old catalog, and its rationale
("an execute needs a person; a context tool names the caller") no longer
describes anything.

### What replaced it

**The parity test kept its subject and lost its exclusion.** Every one of the
22 tools has a CLI equivalent, so `EQUIVALENTS` covers all 22 and the
assertion is now:

```rust
let covered: Vec<&str> = EQUIVALENTS.iter().map(|(tool, ..)| *tool).collect();
assert_eq!(covered, CATALOG, "a tool has no command behind it");
```

**The invariant is asserted in two new places**, each written so that a future
PR adding a write tool fails *the invariant*, not merely an allowlist:

`mcp::catalog::tests::every_tool_in_the_catalog_is_a_read` (unit, in-process):

- `spec.effect == Effect::Read` for every tool;
- `spec.tool().annotations.read_only_hint == Some(true)`;
- no name ends with `.prepare`, `.execute`, `.acknowledge`, `.reconcile`,
  `.run`, or `.plan`;
- no name starts with `context.`.

`mcp::the_tool_list_is_the_report_catalog_and_every_tool_is_a_read`
(integration, over the wire from `tools/list`): the same three checks on what
a host actually receives, plus `destructiveHint: false`, the two-branch
`outputSchema`, and a widened forbidden-argument list that now also rejects
`assume_not_submitted`, `assume_not_posted`, and `generation`.

`mcp::no_removed_tool_is_reachable_and_no_tool_asks_for_an_approval` (new):

- each of the 21 removed names returns `-32601` and is absent from `CATALOG`;
- a `requestState` sent with `todo.list`, `courses.list`, `receipts.list`
  returns `-32602` and the tool does not run;
- every request the mock server saw during the test was a `GET`.

`no_tool_reaches_a_forbidden_surface` gained substring checks for `submit`,
`send`, `reply`, `download`, `sync`, `open`, `note` — so a re-added write tool
trips on its name even before the shape checks.

---

## 4. Dead code removed, and why

Removing the MCP wiring made roughly 500 lines of CLI-side plumbing
unreachable. `cargo check --workspace --all-targets` was clean at `9394895`
(verified: 0 warnings) and produced 26 warnings after the catalog change.
`clippy -D warnings` is a required gate and `#[allow(dead_code)]` was
explicitly forbidden, so the only way to stay green was to remove it.

**No CLI command's behaviour changes.** Every item below was reachable *only*
from the MCP dispatch — verified by `cargo check` before and after.

| File | Removed | Why it was dead |
|---|---|---|
| `commands/submit.rs` | `Pending`, `Admitted`, `Refusal`, `agent_prepare`, `agent_execute`, `agent_approve`, `agent_refuse`, `agent_approval_required`, `is_operation_plan`, `plan_summary`, `render_plan`, `OnExisting`, `replayed_journal` | the whole "agent surface" block; its only callers were `submission.prepare`/`.execute` and `server.rs` |
| `commands/operation.rs` | the same `agent_*` set, `plan_summary`, `OnExisting`, `replayed_journal` | mirror of the above for the three write tools |
| `commands/here.rs` | `attach` | only caller was `context.attach` |
| `commands/bridge.rs` | `detach_consumer` | only caller was `context.detach`; the CLI's own `detach` stays |
| `commands/open.rs` | `Launch` enum and the `browser: Launch` parameter | only `Launch::No` came from `open.url`; `canvas open` always passed `Yes`, so `handle` now always launches |

`OnExisting` and `replayed_journal` deserve a note. `OnExisting::Refuse` is
what `canvas submit` and the three write commands always used;
`OnExisting::Replay` was constructed only inside `agent_execute` /
`agent_approve`. With `Replay` gone the enum had one variant and
`replayed_journal` had no caller, so both went and `run_plan` lost its
`on_existing` parameter. The `Admission::Existing` arm now returns the same
exit-8 refusal it always returned for a CLI caller — byte for byte the same
code path, one fewer branch.

Unused imports were trimmed in `catalog.rs` (`PathBuf`, 8 command modules, 12
`SCHEMA_*` constants), `operation.rs` (`OperationTarget`, `PlanState`,
`PlanJson`, `PlanResult`, `SCHEMA_PLAN`, `plan_refusal_with`, `render_plan`)
and `submit.rs` (`PlanState`, `PlanJson`, `PlanResult`, `SCHEMA_PLAN`).

**Nothing in `mcp/` is still reachable-but-dead**, and nothing was kept for a
"legitimate reason" that needs reporting — except the `context/` resource
described in section 8, which is reachable and answers, but can no longer
answer anything but `not_attached`.

---

## 5. Tests outside `tests/mcp.rs`

`tests/bridge.rs` and `tests/m7b.rs` drove the `context.*` tools directly and
could not compile otherwise. Both are outside the brief's file list; I changed
them because the alternative was a broken build.

**`tests/bridge.rs`** —
`only_the_consumer_that_attached_reads_the_bundle` became
`no_mcp_consumer_can_attach_and_the_context_resource_reads_not_attached`. It
now asserts: all five `context.*` names are `-32601`; the `context/<handle>`
resource answers `not_attached` for alpha's own handle, for beta's own handle,
and for beta naming alpha's handle; no page text ("Essay 1") reaches MCP in
any of the three; and the person's own `canvas here` still reads the attached
tab. The M7-a invariant "a consumer handle is not a name anyone may read
under" is still covered.

**`tests/m7b.rs`** —
`a_note_is_bound_to_the_generation_it_was_written_against` keeps its exact
acceptance criterion but drives it through `canvas note --generation N`, which
reaches the identical `note::handle`. Generations 1 and 3 are exit 8
`stale_generation`; generation 2 is exit 0.

`a_stale_follow_is_refused_before_the_browser_is_asked` became
`a_refused_follow_never_reaches_the_browser`. The cross-origin resolution
refusal is kept via `canvas open --follow https://evil.test/...` (exit 6) plus
the "browser was never asked" assertion. **The stale-generation half is gone
and could not be preserved** — see section 8.

**`tests/skill.rs`** — `CATALOG` trimmed to 22; a `REMOVED` list of 21 added;
the two workflow-exclusivity tests
(`the_submission_tools_are_only_named_by_the_approval_workflow`,
`the_write_tools_are_only_named_by_the_reply_workflow`) removed, because their
subject was which workflow may name a write *tool*. Four tests added:
`no_workflow_names_a_removed_tool` (substring match anywhere in any skill file,
prose included), `every_write_workflow_routes_through_the_cli`,
`the_reply_workflow_states_the_course_policy_boundary` (keeps the REPORT §3.5
assertions the deleted test carried), `the_skill_states_that_the_catalog_is_read_only`.

**`xtask/src/bench_mcp.rs`** — the required gate `cargo xtask bench --mcp`
**failed outright** before this change:

```
bench failed: tools/call failed: {"code":-32601,"message":"unknown tool download.plan"}
```

The harness's workflow table mirrored the skill and called `download.plan`. It
now issues only the tool calls each workflow makes and counts each write as
the `canvas` command it is, with the unmeasured notes rewritten to say so.
The numbers in section 9 come from the fixed harness.

**`README.md`** — its agent section claimed "The catalog is read-first … A
submission still needs a recorded human approval." There is no submission tool
at all now. Rewritten to state the read-only rule and that every write is a
`canvas` command. Also corrected "five workflows" → "six" (a pre-existing
error; the skill has shipped six since M8-b).

---

## 6. The shipped skill — all six workflows kept, none removed

The brief asked me to read all the shipped workflows and, per workflow, either
rewrite to the CLI equivalent or remove, and to say which and why. There are
six files (the brief said five; SPEC §21.3 and `tests/skill.rs` both say six,
and six ship).

**Every one was rewritten. None was removed.** In all four write cases,
rewriting made sense for the same reason: the *reads* in the workflow still
happen over MCP, and the write is a command a skill can shell out to. Removing
the workflow would leave a model with no route at all to something the product
still does.

| Workflow | Verdict | What changed |
|---|---|---|
| `organize-the-week.md` | **Rewritten** | `sync.run` → "ask the user to run `canvas sync`"; `open.url` → the Canvas URL the read already carries. Two call-block lines dropped. The cost note now talks about `canvas sync`. |
| `read-an-assignment.md` | **Rewritten** | `open.url` → the URL in `assignment.get`. One step and one call line. |
| `prepare-and-submit.md` | **Rewritten to `canvas submit`** | Opens with "You cannot do this over MCP at all"; reads stay tools; `canvas submit --file/--text/--url` is the write; adds **never pass `--yes`**; keeps the exit 8 / 9 / 11 rules; closes with why the decision was made. |
| `reply-and-message-with-approval.md` | **Rewritten to the three write commands** | `canvas discussion reply`, `canvas inbox send`, `canvas inbox reply`, then `canvas operation status` / `canvas operation reconcile`. The REPORT §3.5 course-policy boundary, the attribution ladder and the "accepted is not delivered" rule are all kept verbatim. Adds **never pass `--yes`**. |
| `reconcile-an-unknown-outcome.md` | **Rewritten** | `receipts.list`/`receipts.show`/`submission.get` stay tools; `submission.reconcile` → `canvas submission reconcile --json`; `receipts.acknowledge` → `canvas receipts acknowledge`. The `assume_not_submitted` section became `--assume-not-submitted` and now says such an argument belongs at the terminal. |
| `download-course-files.md` | **Rewritten** | `files.list`/`modules.list` stay tools; `download.plan` → `canvas download --dry-run --json`; `download.run` → `canvas download`. Adds "never pass `--force`". |

**`SKILL.md`** — the frontmatter `description` now says the MCP server is
read-only and names the CLI writes. The opening states the read-only rule in
the first screen a model reads, with "not a restricted one, not a gated one,
none … not a gap to work around". "Where the user is" was six numbered
`context.*` tool steps and is now a block of `canvas here` / `canvas here
--text` / `canvas note --generation` / `canvas open --follow` / `canvas bridge
status`, keeping every zone, reason and refusal rule intact. The exit-7 row,
the freshness section and the resources section lost their `sync.run` and
`context.attach` references. A new closing paragraph lists every command the
CLI carries that the catalog does not.

One wording detail: `prepare-and-submit.md` first said "there is no
`submission.prepare`, no `submission.execute`" — an explicit statement of
absence, which is the house style for forbidden flags. But
`no_workflow_names_a_removed_tool` is a plain substring test, and a model
skimming for tool names could still lift them. I changed it to "there is no
prepare tool, no execute tool", which keeps the guidance and the strict test.

---

## 7. SPEC and `writes-v2.md`

### §21.2 — rewritten

- **Tool table**: one row, `Read (22)`, listing the 22 names, `readOnlyHint`
  true.
- **Effect grouping**: collapsed, and stated as an invariant rather than an
  observation — `Effect` has a single variant, so a non-read has no spelling
  in `ToolSpec`. Names the two tests that enforce it.
- **"What the catalog does not contain"**: rewritten to lead with "everything
  that writes", point at §19 item 48 for the 21 names, and note that a removed
  name is `METHOD_NOT_FOUND`.
- **"The approval round trip"** subsection: replaced by **"No approval round
  trip"** — this server never answers `input_required`, never sends an
  `elicitation/create`, never issues a handle, and refuses a `requestState`.
  It states explicitly that the §20 approval mechanism itself is unchanged and
  still required, at the terminal.
- **Resources table**: the `context/{consumer_handle}` row now says it answers
  `not_attached` for every handle and points at §19 item 49.
- **Catalog size**: re-measured, not estimated (section 9).

### §21.3 and §21.4

§21.3 describes the six workflows routing writes through commands, and the new
skill tests. §21.4 no longer claims an approval round trip was verified against
any client.

### §19 item 48 — new

Records the owner directive verbatim ("mcp should only list get tools thats
it"), the 22 kept, all 21 removed by name, and the four that carried
`Effect::Read` and went anyway (`download.plan`, `operation.status`,
`open.url`, `context.here`) with the owner's reasoning. Ends with an explicit
**"No CLI command changed"** list.

### §19 item 49 — new

The three things the narrowing left unreachable. See section 8.

### §19 item 19 — left open, with the reason

**The smaller catalog does not resolve it.** The trim took one of item 19's
three options and it went as far as that option can go: 43 tools / 344 878
bytes / ~86 235 tokens → 22 tools / 167 955 bytes / ~41 997 tokens. But
~41 900 tokens *is the number item 19 was raised about* — the surface is back
to the M6-b cost, not below it. Item 19 now records that the trim resolved the
growth since M6-b and resolved nothing about the per-tool envelope; only the
two schema options left (`$ref`s, or a compact envelope schema per tool) can
move it.

### §19 items 21 and 38 — marked moot

Both asked about `readOnlyHint` on tools that no longer exist
(`download.plan` / `open.url` / `download.run --jobs`, and
`operation.status`). Item 38 is doubly moot: the tool was removed on exactly
the reading that item proposed.

### §20 and §25 — pointer sentences only

Each got one added paragraph saying MCP exposes no tool for it and the
mechanism below is unchanged. No other body text was touched, as instructed.

### §25.10 and §24.14 — rewritten

These two subsections' **entire subject** was the MCP tools (the eight M8-b
tools; the five M7 `context.*` tools). Leaving them would have made the SPEC
contradict §21.2 in two places. §25.10 now lists the five `canvas` commands in
a table and keeps the skill paragraph. §24.14 says no tool reaches the
companion, points at the §24.9 commands, and keeps the resource template rules
plus the §19 item 49 caveat.

### Four other SPEC passages corrected

Each stated the old surface as present-tense fact:

- §12.2 "human `submit`" — "`submission.execute` replays it instead (below)".
- §12.2 `replayed` (§19 item 17) — now says the field is always `false` today
  and why, keeping item 17 open.
- §24 invariants — two bullets describing `context.here`, `context.attach`,
  `context.detach` as tools.
- §24 text release — "`canvas here --text`, or `context.here` with
  `include_text: true`".

### `docs/agent-hosts.md`

The `input_required` column is gone from the host matrix, with a line saying
why (no tool can trigger one) and a line saying a declared elicitation
capability is recorded because the host sent it, not because the server uses
it. The harness bullet no longer claims to exercise accept/decline/cancel/
replay/`approval_required`, and instead names what it does pin. "What stayed
untested" now leads with "Any tool call in a third-party host" and adds
"Every row against the current build".

One thing worth flagging: this file already said "22 tools" for every row. It
was recorded at commit `590aac7`, when the catalog genuinely was 22 read tools;
the catalog grew to 43 afterwards and nobody re-ran the hosts. The counts are
accidentally correct again, and the file now says so plainly rather than
letting the coincidence pass as a fresh measurement.

### `docs/writes-v2.md`

Reduced to the pointer the brief asked for. The §21 link was removed from the
"MCP tools are §21" clause; "the eight tools" dropped from the §25 contents
list; a new paragraph states the writes are CLI-only, names the four commands,
and points at §21, §25 and §19 item 48. The open-items line drops item 38 as
moot.

---

## 8. Three things I did not fix — owner's call (§19 item 49)

**1. The `context/{consumer_handle}` resource can never become attached.**
`context.attach` was the only caller of `here::attach`, so nothing in the
shipped binary sends the broker an `Attach` op for an MCP consumer. The
template is still listed and still answers, but it answers `not_attached` for
every handle, for the life of the build. The resource surface was not in this
package's scope, so I left it and documented it. Options: drop the template, or
give the CLI a way to attach a named consumer so an agent that shells out can
opt in.

**2. The stale-generation refusal on a follow is unreachable.** `open::follow`
still takes a navigation generation and still refuses a stale one, but its one
remaining caller — `canvas open --follow` in `main.rs` — passes `None`. The
guard is correct and dead, and it is not dead *code* (the parameter is used),
so clippy does not complain. Adding `--generation` to `canvas open --follow`
would be a CLI behaviour change, which the brief forbids. The m7b test lost
that half of its coverage as a result.

**3. §19 item 17's replay rule has no surface.** The rule — an execute on a
plan that already admitted a journal returns that journal with
`replayed: true` — was reachable only through `submission.execute` and the
three operation executes. `canvas submit` and the §25 write commands refuse
such a plan with exit 8 instead, which is what they always did. I removed the
replay code with the tools and left item 17 open, because the rule is still the
right one if an approved-plan surface ever returns.

I changed nothing in `extension/`, nothing in `canvas-core`, and no CLI
command's behaviour.

---

## 9. Catalog size, before and after

Measured by `cargo xtask bench --runs 3 --mcp`, which regenerated
`docs/bench.md`. These are the harness's real numbers, not estimates.

| | Tools | Bytes per `tools/list` | ~Tokens (1 per 4 bytes) |
|---|---:|---:|---:|
| Before (M8-b) | 43 | 344 878 | ~86 235 |
| **After (M9)** | **22** | **167 955** | **~41 997** |
| Change | −21 | −176 923 (−51.3 %) | −44 238 (−51.3 %) |
| M6-b, for reference | 22 | — | ~41 900 |

The byte column is exact; the token column is one token per four bytes of
UTF-8, a rule of thumb, not a tokenizer run. `docs/bench.md`'s per-tool table
now lists 22 rows and the workflow table counts each write as an unmeasured
`canvas` command.

The most expensive surviving row is `receipts.show` at 12 846 bytes
(~3 212 tokens); the cheapest is `inbox.unread_count` at 5 846 (~1 462).

---

## 10. Gate results

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | **pass** |
| `cargo clippy --all-targets --all-features -- -D warnings` | **pass**, no `#[allow(dead_code)]` added anywhere |
| `cargo nextest run --all-features` | **919 run, 908 passed, 11 failed** — see below |
| `cargo deny check` | **pass** — advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | **pass** |
| `cd extension && npm test` | **pass** — 52/52, `extension/` untouched |
| `cargo xtask bench --runs 3 --mcp` | **pass** — regenerated `docs/bench.md`; every latency metric `ok` against its p95 target |

### The 11 failures are pre-existing and not caused by this package

I built a detached worktree at `9394895` (the commit this lane started from)
in a separate target dir and ran the same command there:

- baseline `9394895`: **923 run, 912 passed, 11 failed**
- this branch: **919 run, 908 passed, 11 failed**

A `diff` of the two failure name sets is **empty** — the same 11 tests, in both
runs.

They are environment-dependent. Each asserts exit 3 ("no identity") but this
machine has a real stored Canvas identity that the binary picks up, so they get
exit 13 instead:

```
cannot read identity courses.lasell.edu-22592-4ed5c416: identity changed
```

The 11: `cli::command_choices_accept_documented_forms`,
`cli::m1b_commands_exit_auth_without_identity`,
`cli::m4b_commands_need_an_identity`,
`cli::nonraw_variants_continue_to_accept_json`, `cli::todo_is_stub`,
`cli::every_v1_command_is_wired`,
`cli::mixed_commands_accept_typed_and_positional_forms`,
`exit_precedence::courses_offline_json_without_identity_is_auth_exit_3`,
`files_modules::files_auth_without_identity_is_exit_3`,
`files_modules::modules_auth_without_identity_is_exit_3`,
`grades::grades_without_an_identity_is_exit_3`.

The temporary baseline worktree and its target directory were removed;
`git worktree list` is back to the four lanes it started with.

### The 923 → 919 accounting

Net **−4**, and it accounts exactly:

| Change | Δ |
|---|---:|
| `tests/mcp.rs`: removed `a_declined_or_cancelled_approval_dispatches_nothing`, `an_accepted_approval_submits_once_and_replays_after_that`, `a_host_without_elicitation_is_refused_and_nothing_is_dispatched`, `a_write_tool_asks_for_an_approval_and_replies_once`, `an_approval_cannot_be_asserted_by_an_argument` | −5 |
| `tests/mcp.rs`: removed `open_url_resolves_without_launching` (the tool is gone) | −1 |
| `tests/mcp.rs`: added `no_removed_tool_is_reachable_and_no_tool_asks_for_an_approval` | +1 |
| `catalog.rs` units: removed `a_text_entry_cannot_read_the_protocol_channel`, `reconcile_does_not_assume_anything_by_default` (both tested removed arg structs) | −2 |
| `catalog.rs` units: added `every_tool_in_the_catalog_is_a_read` | +1 |
| `tests/skill.rs`: removed `the_submission_tools_are_only_named_by_the_approval_workflow`, `the_write_tools_are_only_named_by_the_reply_workflow` | −2 |
| `tests/skill.rs`: added `no_workflow_names_a_removed_tool`, `every_write_workflow_routes_through_the_cli`, `the_reply_workflow_states_the_course_policy_boundary`, `the_skill_states_that_the_catalog_is_read_only` | +4 |
| **Total** | **−4** |

923 − 4 = 919. ✓

Renames (`the_tool_list_is_the_report_catalog_with_effect_annotations` →
`…_and_every_tool_is_a_read`; the two bridge/m7b renames) do not change the
count.

---

## 11. Files touched

Inside the brief's list:

```
crates/canvas-cli/src/mcp/catalog.rs
crates/canvas-cli/src/mcp/server.rs
crates/canvas-cli/tests/mcp.rs
docs/agent-hosts.md
docs/SPEC.md
docs/writes-v2.md
skill/canvas-cli/SKILL.md
skill/canvas-cli/download-course-files.md
skill/canvas-cli/organize-the-week.md
skill/canvas-cli/prepare-and-submit.md
skill/canvas-cli/read-an-assignment.md
skill/canvas-cli/reconcile-an-unknown-outcome.md
skill/canvas-cli/reply-and-message-with-approval.md
```

Outside it, each for a stated reason:

```
crates/canvas-cli/src/commands/{submit,operation,here,bridge,open}.rs  dead code; clippy gate
crates/canvas-cli/tests/{bridge,m7b,skill}.rs                          drove removed tools
xtask/src/bench_mcp.rs                                                 the --mcp gate failed
README.md                                                              stated a false claim
docs/bench.md                                                          regenerated by the gate
```

`crates/canvas-cli/src/output/json_schema.rs` needed **no** change: the schema
registry is keyed by command, not by tool, and every `SCHEMA_*` the removed
tools named is still printed by a live CLI command.
```
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m9`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M9):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M9.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M9.md`
