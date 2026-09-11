# Code review + fix — M9-b on branch lane/w1 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M9-b
on branch `lane/w1` (worktree `/Users/rolfie/projects/canvas-cli`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /Users/rolfie/projects/canvas-cli`. Read the package brief `tasks/m9b-mcp-single-tool.md` and the SPEC sections it
   cites. Then read `git log main..HEAD --stat` and the full diff (11 commits from `main`: the M9 22-tool cut, then the M9-b single-tool collapse on top of it). Contract: the owner's own words, quoted verbatim in the new SPEC §19 item: "i want mcp to be one tool getclitools and it shows how to use the cli tools" and "drop both, mcp is just the one tool for the cli thats it" ("both" = the browser-context resource and the event subscriptions). Confirmed final shape: `canvas mcp` serves exactly ONE tool, named `getclitools`, whose result is the complete `canvas` command reference built on `canvas schema`'s existing machinery; nothing else. Package briefs: `tasks/m9-mcp-reads-only.md` then `tasks/m9b-mcp-single-tool.md`. Touch nothing under `extension/`, `canvas-core`, or any CLI command's own runtime behavior. Attack it as an attacker and as a completeness check, both: (1) Confirm `tools/list` over a real `canvas mcp` session (or the equivalent test) returns EXACTLY one tool, named `getclitools`, and that calling it returns the actual command reference, not a stub — spot-check that several real commands (including at least one write command like `discussion reply` or `submit`, and at least one companion command like `bridge`) appear in the response with correct flags, not just the read commands. (2) Grep the whole tree for every removed tool name (the 21 from M9, and any reference to `context.attach`, `context.here`, `context.detach`, `context.note`, `context.follow`, `subscriptions/listen`, `context/{consumer_handle}`) across `mcp/`, `tests/mcp.rs`, `skill/canvas-cli/`, `docs/agent-hosts.md`, `docs/SPEC.md`, `docs/writes-v2.md`, `docs/reads-v2.md`, `docs/companion.md`, `docs/bench.md`, the README — a stray mention in a doc still telling an agent to call a tool that no longer exists is a defect. (3) Read `crates/canvas-cli/src/mcp/resources.rs` and `subscribe.rs` in git history (`git show main:...`) versus their absence now, and confirm nothing genuinely load-bearing went with them — in particular, re-verify the report's claim that the `context/{consumer_handle}` resource was already permanently unreachable (§19 item 49 point 1) BEFORE this deletion, by checking whether `context.attach` truly had no other caller in the M9-cut code; if the report is wrong about that, this is a functional regression, not a cleanup. (4) Confirm the CLI itself is completely unchanged in behavior: diff `main...HEAD` for `crates/canvas-cli/src/commands/`, `crates/canvas-core/`, and `extension/`, and treat any behavioral change there as a defect unless the report's §11 'Deviations' section justifies it in writing (it names one CLI-adjacent change, `here.rs::foreign_consumer` removal — verify that function truly had no other caller). (5) Verify the `canvas schema` extension described in report §3.1 — read exactly what was added to make it cover commands agents need (writes, `bridge`, etc.) and confirm it did not change `canvas schema`'s existing output shape for any command a human or existing test already relies on (a schema page gaining new coverage is fine; an existing page's shape changing is a defect unless justified). (6) Verify the size numbers (729 bytes / ~183 tokens for `tools/list`) yourself — rerun `cargo xtask bench --mcp` and read `docs/bench.md`, don't trust the report's copy of the number. (7) Verify the exact test-count accounting (919 → 891, net −28) the same way the M9 round's was verified: read the actual diff of test names removed/added/renamed in `tests/mcp.rs`, `mcp/reference.rs`, `tests/bridge.rs`, and the skill tests, and confirm the arithmetic, not just the total. (8) The report says the same 11 pre-existing environment-dependent test failures recur (identity-related, unrelated to this package) — spot-check two or three of them against a fresh checkout of `main` in your own reviewer target dir rather than trusting the report's claim. (9) Read `docs/companion.md`'s changed manual-check row (report §7: the two-consumer check can no longer be run from MCP) and confirm the doc is honest about why, not just quietly reworded. (10) Read the three 'Left open' items in the report (§19 item 49 points 2-3, no third-party host re-run, the item-50 cost) against the actual code — confirm each is genuinely something for the owner to decide, not a shortcut for a real defect. The worker's final report, for reference:

```
# M9-b — `canvas mcp` is one tool

Lane w1, worktree `/Users/rolfie/projects/canvas-cli`, branch `lane/w1`.
Built forward on the four M9 commits; nothing was reverted.
Nothing pushed. Nothing merged into `main`.

```
2b4d6c1 chore: untrack a stray session note committed by mistake
f4584bc docs(companion): the two-consumer check cannot be run from MCP any more
852b853 docs(bench): the final MCP run of this package
47d2ad5 docs(spec): §21 is a discovery tool for a CLI the agent runs itself
d3ad7a0 docs(skill): every step is a `canvas` command, reads included
6292f41 test(mcp): pin the one tool, and what it answers with
bccce10 feat(mcp)!: one tool, getclitools, and nothing else
```

Package diffstat against the M9 tip (`9534ae5`):
`34 files changed, 2424 insertions(+), 4257 deletions(-)`.
Two of those files are the brief itself (`tasks/m9b-mcp-single-tool.md`) and the
M9 code review (`tasks/review-code-m9.md`), which arrived with the assignment.
The only source file this package **adds** is
`crates/canvas-cli/src/mcp/reference.rs`.

---

## 1. What the surface is now

`canvas mcp` serves exactly one tool, named `getclitools` verbatim. It performs
nothing. It returns one text block: the whole `canvas` command reference.
Everything the agent does after that is a `canvas` command it runs itself.

| | M8-b | M9 | M9-b |
|---|---:|---:|---:|
| Tools | 43 | 22 | **1** |
| Resources (listed + templates) | 4 | 4 | **0** |
| Subscriptions | `subscriptions/listen` | `subscriptions/listen` | **none** |
| Capabilities advertised | tools, resources, list-changed, subscribe | same | **tools only** |
| `tools/list` result bytes | 344 878 | 167 955 | **729** |
| `tools/list` result tokens (÷4) | ~86 235 | ~41 997 | **~183** |
| Tool definitions alone | 344 878 | 167 955 | **656 B / ~164 tok** |
| The one answer | — | — | **32 420 B / ~8 105 tok, 75 commands** |

`tools/list` is **230× smaller** than the M9 catalog and **473× smaller** than
the M6-b/M8-b one. The 729-byte figure is the whole JSON-RPC result, measured
on the wire by `tests/mcp.rs`; the 656-byte figure is the tool definition alone,
measured by `cargo xtask bench --mcp`.

---

## 2. The exact shape of `getclitools`

### 2.1 The tool definition

`crates/canvas-cli/src/mcp/catalog.rs`, 1846 lines at M8-b → 1004 after M9 →
**190 now**.

| Field | Value |
|---|---|
| `name` | `getclitools` — the owner's own word, verbatim, not bent into the dotted convention the removed catalog used |
| `title` | `Get the canvas CLI tools` |
| `description` | "Get the complete `canvas` command-line reference: every command, its operands and flags, and the JSON envelope each one returns. Call it once, then run `canvas <command> ...` yourself in a shell. This is the only tool this server has: nothing here reads or writes Canvas." |
| `inputSchema` | `GetCliToolsArgs {}` — an object with no properties and `deny_unknown_fields` |
| `outputSchema` | **absent** |
| `annotations` | `readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true`, `openWorldHint: false`, plus `title` |

`ToolSpec` kept its name, as the brief asked, but only the four fields a
discovery tool has: `name`, `title`, `description`, `input_schema`. The fields
that described a command behind a tool — `schema`, `variant`, `effect`,
`idempotent`, `open_world` — went with the commands.

`openWorldHint: false` and `idempotentHint: true` are literally true here: the
answer is built from the binary's own command tree, with no session, no network,
and no cache. The same bytes every time, reaching nothing. A test asserts the
server received no HTTP request while answering.

**No `outputSchema`.** The answer is prose, not a §7 envelope, so there is
nothing for a host validator to check. That single fact is most of the 230×
saving, and it also closes the Cursor defect recorded in `docs/agent-hosts.md`
in the strongest possible way: there is no `outputSchema` to mishandle.

### 2.2 What the answer contains

`crates/canvas-cli/src/mcp/reference.rs`, **new, 402 lines**. One
`pub fn reference() -> String`. Structure of the document it builds:

1. **Preamble.**
   - `# The `canvas` command line` — "This is the whole surface… there is no
     second tool, no resource, and no subscription: from here on, run
     `canvas <command> ...` yourself with whatever shell or execution
     capability you have."
   - What the tool is not: a student tool bound to one identity, no teacher/TA/
     admin features, cannot reveal a credential, change an identity, run
     arbitrary HTTP or shell, or clear the cache.
   - **Reading an answer** — the §7 envelope printed in full as JSON, then the
     reading order (`outcome`, `exit`, `partial`/`warnings`, `freshness`,
     `result`), the "a refusal is an answer" rule, the `null` rule, the grades
     rule, and the "never invent a field" rule pointing at `canvas schema`.
   - **Exit codes** — the full §14 table, 0 through 13, each with what to do.
   - **Writes** — writes are commands, each prints what it is about to do and
     asks a person at the terminal, and **"Never pass `--yes`."**
   - **Global flags** — `--json`, `--profile`, `--offline`, `--fresh`,
     `--color`, `-q`, `-v`, listed once instead of 75 times.
2. **`## Commands`** — one `### canvas <path>` section per command, depth-first
   over the whole clap tree, hidden commands and `help` skipped. Each section
   carries: the `about` line, a usage line, `Operands:` with each positional and
   its help, `Options:` with each non-global option, its value name, its
   accepted values and its default, `Subcommands:` where there are any, and a
   `Returns:` line.
3. **`## Schema names`** — the `canvas schema --list` listing, verbatim, in a
   fenced block with its `name\tschema\tkind` header.

One command as the tool actually answers it:

```
### canvas discussion reply

Reply to a discussion topic (needs an approval)

    canvas discussion reply [OPTIONS] <COURSE> <DISCUSSION>

Operands:
  <COURSE>               Course id, code, or alias
  <DISCUSSION>           Discussion id, or a Canvas discussion URL
Options:
  --to <TO>              Reply to this entry instead of the topic
  --text <TEXT>          The reply text, or `-` to read it from stdin
  --text-file <TEXT_FILE> Read the reply text from this file
  --attach <ATTACH>      File to attach (refused in this version)
  --yes                  Skip confirmation
Returns: `canvas-cli/operation@1` with `--json`, `result` fields: acknowledged_at,
attachments, attribution, delivery, error, journal_id, kind, not_posted_evidence,
outcome, plan_id, post_status, readback, receipt_id, replayed, response,
response_kind, server_match, state, subject, target, text.
Full schema: `canvas schema "discussion reply"`.
```

Two rules the rendering had to learn:

- A boolean flag gets no "One of: …" line. Possible values are printed only
  when `takes_a_value(arg)`, or every `--json` in the document would have
  claimed to accept `true` or `false`.
- A command that is only a group of subcommands (`canvas auth`,
  `canvas identity`, `canvas cache`, …) gets **no** `Returns` line. It runs
  nothing of its own, and `is_subcommand_required_set()` is how the tree says
  so.
- A command with no §7 envelope says
  `Returns: raw output. This command writes no §7 envelope, so it has no schema (SPEC §7).`
  rather than promising one.

---

## 3. What `canvas schema` machinery it reuses

Nothing in the reference is written by hand twice. It joins the two
self-descriptions the binary already carries:

| Part of the answer | Source | Also backs |
|---|---|---|
| Commands, subcommands, operands, flags, defaults, value hints | `canvas_cli::dist::command()` — the clap tree the binary itself parses | the man pages, the shell completions |
| What each command returns: schema id, `--json`/`--jsonl`, `result` field names, the `canvas schema "<name>"` that prints it | `crate::output::document_for_command(name)` | `canvas schema <command>` |
| The closing listing | `crate::output::schema_list()` | `canvas schema --list` |

So a command cannot be described to an agent differently from the way it
behaves, and `canvas schema` and `getclitools` describe the same CLI the same
way. `tests/mcp.rs` proves the last row on the wire: it runs
`canvas schema --list` through the fixture CLI and asserts the tool's answer
**contains that stdout byte for byte**.

### 3.1 Was `canvas schema`'s coverage extended? Yes — and how

**This is the one place the package went past the brief's "registry entries are
unaffected" sentence, and it did so on the brief's own explicit instruction:**
"If `canvas schema` cannot currently describe a command that agents actually
need to run (a write command, `bridge`, etc.), extend what it covers rather
than inventing a second, parallel description format." I took the specific
instruction over the general scope line, and I name it here rather than bury it.

The problem was real. `canvas schema "discussion reply"` exited 6. So did
`inbox send`, `inbox reply`, `alias set`, `alias remove`, the three `open`
subcommands, and `auth token`. The registry's own comment already stated the
intent — "One schema for four commands… `command` names the one a person is most
likely to look up, and `canvas schema` resolves the others through it" — but
nothing implemented it. Without a fix, `getclitools` would have had to either
say nothing useful about the three writes an agent most needs, or invent a
second description format for them.

**What I deliberately did not do: add registry entries.** New `SchemaEntry`
rows would duplicate fixtures, collide with
`every_registered_schema_has_a_parseable_fixture`'s `(id, variant)` uniqueness
assertion, and grow the `registered_envelopes` insta snapshot.

**What I did:** `output/registry.rs` gains a nine-row `COMMAND_ALIASES` table —
a command name and the schema id it prints — consulted by `entry_for_command`
after both existing lookups, exposed by `command_aliases()` and
`alias_command()`, listed by `json_schema::list()` as kind `command`, and used
by `document_for_command` so the returned document echoes the name it was asked
for.

| Alias | Resolves to | Why |
|---|---|---|
| `discussion reply`, `inbox send`, `inbox reply` | `operation@1` | the three M8-b writes print the operation journal that `operation status` prints |
| `alias set`, `alias remove` | `alias@1` | every `alias` subcommand prints the same listing |
| `open assignment`, `open file`, `open announcement` | `open@1` | every `open` subcommand prints the same launch result |
| `auth token` | `auth_status@1` | without `--reveal` it *is* `auth status`; with it, raw output |

`alias_command()` returns `None` when a real entry already owns the name, so an
alias can never shadow an entry.

**Nothing else changed.** No fixture, no result shape, no command output, no
snapshot. `canvas schema --list` has nine more rows. A genuinely raw-output
command — `notify`, `completions`, `schema`, `config edit`, `bridge host` — is
deliberately absent and still exits 6, because there is no `--json` contract to
describe; `getclitools` says "raw output" rather than guessing.

Pinned by `tests/schema_cmd.rs::a_command_that_shares_a_shape_answers_under_its_own_name`
(six aliases resolve and echo their own `command`; the five raw-output commands
still exit 6) and by two unit tests in `output/json_schema.rs`.

---

## 4. What was deleted, and what was kept

### 4.1 Deleted entirely — judged genuinely dead

| File / item | Lines | Why it was dead |
|---|---|---:|
| `crates/canvas-cli/src/mcp/resources.rs` | 322 | the whole resource namespace: `todo`, `receipts`, the `course/{id}/assignments` template, and the `context/{consumer_handle}` template §19 item 49 had already recorded as permanently unreachable |
| `crates/canvas-cli/src/mcp/subscribe.rs` | 457 | `subscriptions/listen`, the `_meta` cursor, the invalidation map, the filter acceptance |
| `crates/canvas-cli/src/mcp/result.rs` | 220 | the §7-envelope → `CallToolResult` mapping, `structuredContent`, and the freshness → `ttlMs`/`cacheScope` budget. No tool returns an envelope now, and its only other caller was `resources.rs` |
| `catalog::Effect` + its `annotations()` | ~30 | the effect annotation system. It had already collapsed to one variant in M9; with one tool there is nothing to classify |
| 22 argument structs, 22 `dispatch` arms, `output_schema()`, the `Globals`/`Handled` plumbing | ~800 | their tools are gone |
| `output::document_for_schema` + its re-export | 4 | its only caller was `catalog::output_schema` |
| `commands::here::foreign_consumer` | 23 | its only caller was the `context/` resource. `cargo check` found it — this was the **one** dead-code warning the whole deletion produced |
| `tests/support/Mcp::tool` | 9 | already unused, and it read `structuredContent`, which nothing returns now |
| server: `binding` field, `globals` field, `consumer_of`, `ANONYMOUS_CONSUMER`, `list_resources`, `list_resource_templates`, `read_resource`, `accepted_subscription_filter`, `listen` | ~120 | resource and subscription wiring |

`mcp/server.rs`: 566 lines at M8-b → 373 after M9 → **237 now**. `CanvasServer`
is now a unit struct holding no state at all: the identity binding it used to
carry existed only to build resource URIs.

`clippy --all-targets --all-features -- -D warnings` is green with **no
`#[allow(dead_code)]` added anywhere**.

### 4.2 Kept — judged load-bearing, not leftovers

| Kept | Why |
|---|---|
| Both protocol revisions, `initialize`, `server/discover`, `tools/list`, `tools/call` | the handshake is the surface |
| The explicit unknown-revision refusal in `initialize` | without it the rmcp SDK silently downgrades to its newest legacy version, pretending to speak something the host did not ask for |
| `SURFACE_TTL_MS` (60 s) and `CacheScope::Private` on `discover` and `list_tools` | the revision puts `ttlMs`/`cacheScope` exactly there, and one instance serves one identity, so the scope is never shared |
| The `requestState` refusal in `call_tool` | eight lines. Nothing on this surface asks for an approval, so a state can only be a host bug or a replay; refusing it keeps a replayed decision from being recorded against anything. Tested on the wire |
| `Binding`, `bind`, `still_bound`, `watch_identity` — moved from `resources.rs` into `mcp/mod.rs` | §10, not MCP plumbing. `canvas mcp` still refuses to start without an identity (exit 3) and still stops when the generation changes (exit 13) |
| `output::entry_for_schema` | still used by `json_schema.rs`'s own tests for the stream schemas |

The `_meta` cursor and TTL handling **on tool results** went with `result.rs`:
`dev.canvas-cli/ttlMs` and `dev.canvas-cli/cacheScope` existed to describe
envelope freshness, and there are no envelopes on this surface. The `ttlMs` and
`cacheScope` on **discovery and `tools/list`** are a different thing and stayed.

---

## 5. The skill rewrite — reads now route through the CLI too

M9 routed the writes through the CLI. This package did the reads. All six
workflows and `SKILL.md` were rewritten.

`SKILL.md`:

- Leads with **"## If you are connected over MCP"**: one tool, `getclitools`,
  it performs nothing, call it once at the start, then run
  `canvas <command> ...` yourself. It quotes the owner — "mcp is just the one
  tool for the cli thats it" — so a model reads this as a decision, not a gap
  to work around. "If you have no shell, say so and give the user the command
  line to run. Do not look for another tool; there is not one."
- The **Resources** section is deleted. There are none.
- The `_meta` cache-hint paragraph is deleted. No tool returns an envelope to
  carry them.
- The frontmatter `description` now says the skill reads **and writes** through
  the command line, and names the write verbs.
- Kept and unchanged in substance: the §7 envelope and its reading order, the
  identity section, the companion section, the exit-code table with its two
  overriding rules, the freshness/cache section, and the MCP setup blocks for
  Claude Code, Codex, and Cursor (now saying "The server's whole surface is
  `getclitools`").

The six workflows, and the commands their steps now name:

| Workflow | Commands the steps run |
|---|---|
| `organize-the-week.md` | `canvas courses`, `course`, `todo`, `calendar`, `announcements`, `announcement`, `grades`, `inbox unread-count`, `inbox --scope`, `inbox show`, `discussions`, `discussion`, `sync` |
| `read-an-assignment.md` | `canvas assignments`, `assignment`, `submission`, `syllabus`, `pages`, `page` |
| `prepare-and-submit.md` | `canvas submit`, `receipts list`, `receipts show` |
| `reply-and-message-with-approval.md` | `canvas discussion reply`, `inbox send`, `inbox reply`, `operation status`, `operation reconcile` |
| `reconcile-an-unknown-outcome.md` | `canvas submission reconcile`, `receipts acknowledge`, `receipts show` |
| `download-course-files.md` | `canvas files`, `modules`, `download` |

`tests/skill.rs` no longer diffs a catalog against prose. It pins, per file:

- that no file in the package names **any** of the 43 tool names that ever
  existed;
- that each workflow names the specific `canvas ...` commands its steps need
  (13 substrings for `organize-the-week.md`, 9 for `read-an-assignment.md`, and
  so on) and carries at least one ```` ```sh\ncanvas ```` block a model can
  copy;
- that `SKILL.md` states the one-tool surface in its own words — "one tool,
  `getclitools`", "no resource, and no subscription", "Never pass `--yes`",
  "run `canvas <command> ...` yourself";
- that **no invented dotted tool name appears anywhere**: a scan refuses any
  inline code span matching a dotted-lowercase identifier, so a future edit
  cannot quietly reintroduce `todo.list`;
- that the reply workflow still states the course-policy boundary, that the
  envelope and exit codes are still documented, and that the skill is shipped
  and referenced.

---

## 6. SPEC §21 and §19

### §21 — reframed, not renumbered

- **§21 intro** now says the agent surface *is* the command line, with a
  three-row table: `canvas schema` (what a command returns), `canvas mcp` (one
  tool that hands over the reference), and the skill (how to use it) — one CLI
  described three ways.
- **§21.2** rewritten: the one tool, what its answer contains, **what the
  surface does not contain** (no second tool, no resource, no subscription, no
  `outputSchema`, no approval round trip), the measured sizes, and why the tool
  is annotated as a closed idempotent read.
- **§21.3** rewritten: every workflow step is a command; the skill is the same
  content the tool returns, in a form a host loads without connecting.
- **§21.4** — the host table. Both third-party rows are kept and explicitly
  marked as predating this round.
- **§22 intro** opens with **"Nothing in this section was removed by M9-b"**:
  the event log, the coordinator, the leases, `canvas watch`, and
  `canvas notify` are unchanged and fully supported. Only the MCP path *into*
  the log is gone.
- **§22.5** rewritten as "There are none": the subscription existed to
  invalidate resources, there are no resources, and following the log is
  `canvas watch --jsonl`.
- Also updated for consistency: §13, the §24 architecture diagram (loses
  `canvas mcp (context.*)`), §24.8 (the consumer-handle rule is past-tensed
  with the reason it is kept), §24.11 and §24.12 (`context.note` /
  `context.follow` are gone), §24.14 (the "one resource template stays
  readable" line becomes the deletion), §24.16 (what `tests/bridge.rs` drives
  now), §25.10 ("what an agent can do over MCP is read the thread" becomes
  "reading the thread is a command too"), and the dependency and ledger tables.

### §19 — the new item and the supersede notes

**Item 50 — "`canvas mcp` is one tool, by the owner's direction (2026-09-10)"**
is new. It records:

- The owner's words verbatim, both of them: *"claude see i don't think you
  understood i want mcp to be one tool getclitools and it shows how to use the
  cli tools."* and, asked about the browser-context resource and the event
  subscriptions, *"drop both, mcp is just the one tool for the cli thats it."*
- All 22 read tool names that left, plus the four resource names and
  `subscriptions/listen`.
- **The reframing**: MCP is no longer a curated catalog of agent-facing actions
  beside the CLI; it is a discovery tool for a CLI the agent runs itself. The
  premise is that an agent which can call an MCP tool can also run a binary.
- **The cost, stated plainly**: "A host with no execution capability can now do
  nothing but show the user a command line", where before it could at least
  read. Recorded as the owner's accepted trade, not re-argued as a question.
- **No CLI command changed** — with the one *extension*: the nine
  `canvas schema` names, listed by name, with the note that no fixture, result
  shape, or command output moved, and that this is the "extend what it covers
  rather than inventing a second, parallel description format" the package was
  asked for.
- **The answer to item 19** (the schema cost of the agent surface): 656 bytes
  for one tool definition against 167 955 and 344 878; the reference is 32 420
  bytes and an agent pays it once per session rather than once per tool
  definition. Item 19 asked whether the per-tool envelope inlining was worth
  its cost — there are no per-tool envelopes left. **Item 19 is resolved.**

**Items 48 and 49 keep their full original text.** Each gains one italic note:

- Item 48: *"Superseded by `bccce10` (M9-b, 2026-09-10) — see item 50: the
  catalog is not 22 tools, it is one, and the 22 reads left with the 21 writes.
  The text above stands as the record of the first cut; what it says about the
  CLI being untouched is still true."*
- Item 49: *"Superseded by `bccce10` (M9-b, 2026-09-10) — see item 50. The
  first point is settled the way it was raised: the `context/{consumer_handle}`
  resource is deleted, along with the whole resource namespace and
  `here::foreign_consumer`, which served nothing else. The second and third
  points are unchanged and still the owner's to rule on: `open::follow`'s
  stale-generation guard is still correct and still dead, and item 17's replay
  rule still has no surface."*

Nothing was rewritten out of the ledger. The record of the first cut stands
beside the correction.

---

## 7. The `docs/companion.md` two-consumer finding

The manual-check table asked a tester to do something that can no longer be
done:

```
| Two consumers | Attach from two MCP hosts | Each reads only after its own `context.attach` |
```

There is no `context.attach` tool since M9, and since M9-b no
`context/{consumer_handle}` resource either, so **no MCP host can attach at
all**. The row named a check with no way to run it. It now names the form that
can be run, and says why it changed:

```
| Two consumers | Run `canvas here` from two terminals under different profiles | Each reads only after its own attach; no MCP host can attach at all since 2026-09-10 (SPEC §19 items 48 and 50) |
```

One row, one line changed. The rest of the check table, and the rest of
`docs/companion.md`, is untouched. This is the one place where narrowing the
agent surface removed a *verification* rather than a feature, so it is called
out here rather than folded into the doc list.

---

## 8. Tests

### 8.1 `crates/canvas-cli/tests/mcp.rs` — rewritten, 1618 → 542 lines, 15 → 6 tests

Gone with their subjects: the catalog allowlist, the M9 read-only invariant,
the two envelope-equivalence tables (`EQUIVALENTS`, `M8A_READS`), the
domain-failure test, the resource-privacy test, and the four subscription and
cursor tests, plus the fixture machinery that primed a cache for them.

The six that remain:

1. **`both_revisions_handshake_and_an_unknown_one_is_refused`** — `2026-07-28`
   and `2025-11-25` both negotiate, an unknown revision fails the handshake,
   and `capabilities.resources` is **absent**.
2. **`the_tool_list_is_one_tool_named_getclitools_and_it_is_small`** — exactly
   one tool, its name, all four annotations, no `outputSchema`, no input
   properties, `cacheScope: private`. It then **measures** the response and
   prints it: `tools/list result: 729 bytes, ~183 tokens`. The ceiling is 2 000
   bytes, so a regression names its own number.
3. **`getclitools_returns_the_canvas_command_reference`** — spot-checks 23
   commands by heading (`### canvas todo`, `### canvas discussion reply`,
   `### canvas notify`, `### canvas watch`, …), checks that operands and flags
   are described, checks four specific `Returns` lines including the `--jsonl`
   one for `watch`, checks that a raw-output command says
   `Returns: raw output.`, runs `canvas schema --list` through the fixture CLI
   and asserts the answer **contains that stdout verbatim**, and asserts
   `server.received_requests()` is empty — building the reference reaches no
   network.
4. **`the_one_tool_is_the_only_name_and_never_asks_for_an_approval`** — all
   **43** names the server has ever served return `METHOD_NOT_FOUND`;
   `resources/read` and `subscriptions/listen` are unroutable; the two list
   methods the rmcp SDK answers by default return empty; three unknown-argument
   shapes and a replayed `requestState` return `-32602`; nothing reached Canvas.
5. **`the_server_refuses_to_start_without_an_identity`** — exit 3, unchanged.
6. **`replacing_the_identity_stops_the_instance`** — exit 13, unchanged.

### 8.2 `mcp/reference.rs` — 4 unit tests

Including one that walks the clap tree and asserts **every** node appears in the
reference under its full path (more than 40 checked), so a command added later
cannot be invisible to an agent.

### 8.3 `crates/canvas-cli/tests/bridge.rs`

`no_mcp_consumer_can_attach_and_the_context_resource_reads_not_attached` drove
`resources/read`, which no longer exists. Rewritten as
`no_mcp_consumer_can_reach_the_browser_and_the_person_still_can`: one tool
listed, all five `context.*` names unroutable, `resources/read` on a
`context/<handle>` URI unroutable for any handle, `resources/list` empty, the
reference naming `### canvas here` and carrying no page content, and the
person's own `canvas here` still reading `browser.title == "Essay 1"` off the
attached tab. Test count unchanged at 9.

### 8.4 Exact test-count accounting: 919 → 891 (net −28)

| Where | Before | After | Δ |
|---|---:|---:|---:|
| `mcp::resources` unit tests | 7 | 0 | **−7** |
| `mcp::subscribe` unit tests | 6 | 0 | **−6** |
| `mcp::result` unit tests | 6 | 0 | **−6** |
| `tests/mcp.rs` | 15 | 6 | **−9** |
| `mcp::catalog` unit tests | 8 | 5 | **−3** |
| `mcp::server` unit tests | 5 | 4 | **−1** |
| `tests/skill.rs` | 9 | 7 | **−2** |
| `mcp::reference` unit tests (new file) | 0 | 4 | **+4** |
| `output::json_schema` unit tests | 11 | 12 | **+1** |
| `tests/schema_cmd.rs` | 7 | 8 | **+1** |
| `xtask::bench_mcp` unit tests | 2 | 2 | 0 |
| `tests/bridge.rs` | 9 | 9 | 0 |
| **Total** | **919** | **891** | **−28** |

Removals sum to −34, additions to +6.

---

## 9. Documentation

| File | Change |
|---|---|
| `docs/agent-hosts.md` | rewritten for the one-tool surface. Resources, subscriptions, and per-tool annotations gone. A new "What the surface is" section. Both third-party rows kept but marked "(then)", with an explicit line: **no third-party row was re-run against the one-tool build**. The Cursor `outputSchema` defect kept as history, with the note that this build has no `outputSchema` at all. A new note that a host also needs a way to run `canvas` itself |
| `docs/SPEC.md` | §21 intro, §21.2, §21.3, §21.4, §22 intro, §22.5, §19 item 50 + supersede notes on 48/49 + item 19 marked resolved, §13, the §24 diagram, §24.8, §24.11, §24.12, §24.14, §24.16, §25.10, the dependency table, the ledger table |
| `docs/reads-v2.md` | the MCP pointer says these reads are CLI-only; MCP exposes no per-action tool at all |
| `docs/writes-v2.md` | same, plus the new `canvas schema "discussion reply"` / `"inbox send"` / `"inbox reply"` names |
| `docs/companion.md` | the two-consumer check row (§7 above) |
| `docs/bench.md` | regenerated by `cargo xtask bench --runs 3 --mcp` |
| `README.md` | the "22 tools, read-only" paragraph becomes the one-tool paragraph; the `canvas schema` paragraph notes that `getclitools` carries the same listing |
| `skill/canvas-cli/*` | §5 above |

`xtask/src/bench_mcp.rs` and `xtask/src/bench.rs` were rewritten to match:
`WorkflowCost`, `workflows()`, and `outcome_of()` are gone (every workflow now
costs exactly one MCP call), replaced by
`ReferenceCost { bytes, tokens, commands }`. The renderer prints "warm
`getclitools` round trip", "What `tools/list` costs", and "What the one call
answers with"; the per-workflow round-trip table is one sentence now.

---

## 10. Gate results

Every gate was run with
`CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/w1`.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | **ok** |
| `cargo clippy --all-targets --all-features -- -D warnings` | **clean**, no new `#[allow]` |
| `cargo nextest run --all-features` | **891 tests run: 880 passed, 11 failed, 0 skipped** |
| `cargo deny check` | **advisories ok, bans ok, licenses ok, sources ok** |
| `cargo +1.88 check --workspace --all-targets` | **ok** |
| `cd extension && npm test` | **52 passed, 0 failed** |
| `cargo xtask bench --runs 3 --mcp` | **every target met**; `docs/bench.md` regenerated |

Bench headline: `warm getclitools over stdio — p50 4.3 ms, p95 4.3 ms, target
100 ms, ok`.

### The 11 failures

They are the **same 11 pre-existing, environment-dependent failures** verified
against a detached-worktree baseline during M9. This machine has a real stored
identity (`courses.lasell.edu-22592-4ed5c416`), so every test that asserts "no
identity ⇒ exit 3" gets exit 13 instead. The failing set is identical to the
M9 baseline set:

```
cli::command_choices_accept_documented_forms
cli::m1b_commands_exit_auth_without_identity
cli::m4b_commands_need_an_identity
cli::nonraw_variants_continue_to_accept_json
cli::todo_is_stub
cli::every_v1_command_is_wired
cli::mixed_commands_accept_typed_and_positional_forms
courses::courses_offline_json_without_identity_is_auth_exit_3
files::files_auth_without_identity_is_exit_3
modules::modules_auth_without_identity_is_exit_3
grades::grades_without_an_identity_is_exit_3
```

None of them touches MCP, the skill, or the schema registry. None is mine.

---

## 11. Deviations from the brief, named

1. **`output/registry.rs` and `output/json_schema.rs` were touched.** The scope
   paragraph says registry entries are unaffected; the `getclitools` paragraph
   asks for `canvas schema`'s coverage to be extended. I took the specific
   instruction and chose the least invasive form — an alias table, not new
   entries — so no fixture, result shape, or snapshot moved. Detail in §3.1.
2. **`xtask/src/bench_mcp.rs` and `xtask/src/bench.rs` were rewritten.** Not in
   the scope list, but the `--mcp` bench **is** in the gate list and could not
   pass otherwise: the old harness called `todo.list` and six workflow tool
   sequences.
3. **`commands/here.rs` lost `foreign_consumer` (23 lines).** CLI-side, but its
   only caller was the deleted context resource, and `-D warnings` would not
   pass with it. No CLI command's behaviour changed.
4. **`docs/companion.md` lost one manual-check row's original wording**, because
   it described a check nobody can run. §7 above.
5. **`ToolSpec` was kept**, as the brief asked, but trimmed to the four fields a
   discovery tool has.
6. **`session-ses_f729.md`** — a scratch note belonging to another session was
   sitting untracked in the worktree and got swept into commit `6292f41` by a
   `git add -A`. Commit `2b4d6c1` untracks it with `git rm --cached`. The file
   is left on disk, untracked, exactly as found. Working tree is otherwise
   clean.

---

## 12. Left open

- **§19 item 49 points 2 and 3** are unchanged and still the owner's:
  `open::follow`'s stale-generation guard is correct and dead, and item 17's
  replay rule has no surface.
- **No third-party host was re-run.** `docs/agent-hosts.md` says so in three
  places. Nobody has watched Claude Code, Cursor, or Codex load this build, call
  `getclitools`, and then go on to run a `canvas` command. That is the one thing
  this design's premise rests on, and it is untested.
- **The cost recorded in item 50**: a host with no execution capability can now
  only show the user a command line. The owner's accepted trade, recorded rather
  than argued.
```
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m9b`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M9-b):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M9-b.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M9-b.md`
