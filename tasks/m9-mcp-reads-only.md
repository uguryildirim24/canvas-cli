# M9 — Narrow the MCP tool catalog to reads only (owner decision, 2026-09-10)

Branch `lane/w1`, worktree `/home/user/projects/canvas-cli`.
`CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/w1`.

## The decision

The owner: "mcp should only list get tools thats it". Confirmed scope: the
`canvas mcp` tool catalog (`tools/list` / `tools/call`) keeps ONLY tools that
fetch Canvas or local read data with no side effect. Everything that
prepares, executes, writes, retires a journal, resolves/navigates, or touches
the browser companion is removed from the catalog.

**This does not touch the CLI.** `canvas submit`, `canvas discussion reply`,
`canvas inbox send|reply`, `canvas operation status|reconcile`,
`canvas download`, `canvas sync`, `canvas bridge *`, `canvas note`,
`canvas open --follow`, `canvas receipts acknowledge` all keep working
exactly as today from a terminal. A terminal coding agent (this one, Codex,
Cursor) calls that binary through shell commands, not through MCP, so none
of those commands, their tests, their registry entries, or `canvas-core`
are in scope for this package. Touch only `crates/canvas-cli/src/mcp/`,
`crates/canvas-cli/src/output/json_schema.rs` if the MCP schema map needs a
matching trim, `crates/canvas-cli/tests/mcp.rs`, `docs/agent-hosts.md`,
`skill/canvas-cli/SKILL.md`, `docs/SPEC.md` §19/§21, `docs/writes-v2.md`
(pointer only — do not touch §25 body text about the CLI writes themselves).

## Keep exactly these 22 tools (in this order)

```
courses.list, course.get, todo.list, assignments.list, assignment.get,
grades.get, files.list, modules.list, pages.list, page.get, syllabus.get,
announcements.list, announcement.get, discussions.list, discussion.get,
inbox.list, inbox.get, inbox.unread_count, calendar.list, submission.get,
receipts.list, receipts.show
```

## Remove exactly these 21 tools

```
sync.run, download.plan, download.run, submission.prepare,
submission.execute, submission.reconcile, discussion.reply.prepare,
discussion.reply.execute, inbox.send.prepare, inbox.send.execute,
inbox.reply.prepare, inbox.reply.execute, operation.status,
operation.reconcile, receipts.acknowledge, open.url, context.attach,
context.here, context.detach, context.note, context.follow
```

`sync.run` and `open.url` are both coded `effect: Effect::Read` today but
neither returns Canvas data (one refreshes the cache, the other resolves a
URL with no fetch); the owner's "get tools thats it" reading drops both for
consistency. `download.plan` and `operation.status` are also `Effect::Read`
today and are dropped anyway per the owner's explicit answer. If you find
another tool whose true effect disagrees with its current `Effect` label,
fix the label as part of this package and say so in your report — do not
silently keep a tool because its annotation says `Read`.

## What to do

1. In `crates/canvas-cli/src/mcp/catalog.rs`: delete the 21 `ToolSpec`
   entries listed above and their matching arms in the dispatch function
   (`execute_tool` or whatever it is named at this call site). Do not touch
   the handler code those arms called into (`submission::prepare`,
   `here::attach`, etc.) — that code is still reachable from the CLI's own
   commands and must keep working. Only the MCP-side wiring goes.
2. Update the catalog allowlist test (`the_catalog_is_the_report_catalog` or
   wherever the exact 43-name list is asserted) to the 22-name list above.
3. Find whatever test currently asserts CLI-versus-MCP parity by excluding
   `.execute` and `context.*` tool names (that exclusion list is now almost
   the whole catalog and its rationale has changed). Replace it with a test
   that asserts the actual invariant going forward: every tool the MCP
   catalog exposes has `effect: Effect::Read` (or equivalent) and
   `readOnlyHint: true`, and that no tool name matches `.prepare`,
   `.execute`, `.acknowledge`, `.reconcile`, `.run`, `.plan`, or starts with
   `context.`. Write it so a future PR that adds a write tool back to the
   catalog fails this test, not just the allowlist.
4. Remove now-genuinely-dead code this leaves behind in `mcp/`: if the
   `input_required` / elicitation approval-round-trip handling in the MCP
   server exists only to service `.execute` tools and nothing in the new
   22-tool catalog can ever trigger it, remove that dead path too (clippy
   `-D warnings` must stay green with no `#[allow(dead_code)]` added to
   paper over it). If any of it is still reachable for a legitimate reason,
   say so in your report instead of guessing.
5. `docs/agent-hosts.md`: drop any claim about testing the approval round
   trip over MCP (there is no longer a tool that can trigger one) and any
   sentence describing MCP as covering writes.
6. `skill/canvas-cli/SKILL.md`: read all five shipped workflows. Any that
   call an MCP write tool now must either be rewritten to call the
   equivalent CLI command instead (skills can shell out) or be removed if
   rewriting doesn't make sense; say which you did and why, per workflow.
7. `docs/SPEC.md`:
   - §21.2 (or wherever the agent surface/MCP catalog section now lives):
     rewrite the tool table, the effect grouping (it collapses — everything
     is `Read` now), the catalog size numbers (re-measure, don't guess), and
     remove the approval-round-trip subsection for MCP specifically (the CLI's
     own approval mechanism in §20 is untouched and still used by `submit`
     etc. run directly).
   - §19: add one new item recording this decision (owner directive,
     2026-09-10, MCP narrowed to reads by design) and note in §19 item 19
     (catalog size) whether the smaller catalog resolves the token-budget
     concern it raised — "resolved by <this package's SHA>" if it clearly
     does, otherwise leave it open and say why.
   - Do not touch §20 (CLI plan/approval) or §25 (CLI discussion/inbox
     writes) body text beyond a pointer sentence saying MCP does not expose
     these; the underlying mechanism and its SPEC description are unchanged.
8. `docs/writes-v2.md`: it currently names MCP write tools; reduce that to a
   pointer saying writes are CLI-only, MCP is read-only by design, and point
   at §21 and §25.

## Gates

`cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D
warnings`, `cargo nextest run --all-features` (expect fewer or equal to 923;
report the exact number and explain any drop), `cargo deny check`,
`cargo +1.88 check --workspace --all-targets`, `(cd extension && npm test)`
(unaffected, must still pass), `cargo xtask bench --runs 3 --mcp` (this
regenerates `docs/bench.md`'s MCP section with the smaller catalog's real
numbers — use those, don't estimate).

Commit as you go, small commits. Never push. Never merge into `main`. Do
not touch `extension/`, `canvas-core`, or any CLI command's own behavior —
if you think one of those needs a change to make this work, stop and name it
in your report instead of making it. `git merge main` first if `main` has
moved (it currently has not, relative to what you start from).

Write your full final report as you finish, then reply exactly:
`DONE M9-mcp-reads-only`
