# M9-b — Collapse the MCP surface to one tool (owner decision, 2026-09-10, supersedes the M9 22-tool cut)

Branch `lane/w1`, worktree `/home/user/projects/canvas-cli`.
`CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/w1`.
Start from the current tip of `lane/w1` (4 commits ahead of `main`, ending in
`docs(spec): §19 item 48 — the MCP catalog is reads only, and what that
cost`). Build forward on those commits; do not revert them.

## The decision, in the owner's own words

"mcp should only list get tools thats it" (M9, already done: 43 tools cut to
22 read-only tools, the approval round trip already removed). Then, when
told the design still exposed 22 separate tools: "claude see i don't think
you understood i want mcp to be one tool getclitools and it shows how to use
the cli tools." Confirmed: "drop both, mcp is just the one tool for the cli
thats it" — "both" being the browser-context resource and the event
subscriptions.

**Final shape.** `canvas mcp` serves exactly **one** tool. Name it
`getclitools` (the owner's own wording, used verbatim as the tool name —
do not rename it to fit a dotted convention). Calling it returns the
complete `canvas` command reference: every command, its flags and operands,
and what it returns, in one response, plus a short preamble telling the
caller to run `canvas <command> ...` itself from here on, through whatever
shell or execution capability it has. **No other MCP tool exists.** The
agent is expected to have direct execution access to the `canvas` binary
(a terminal coding agent already does); MCP's only job is letting it
discover the command surface once.

**Also drop entirely, not just make unreachable:**
- The `context/{consumer_handle}` resource and everything in
  `crates/canvas-cli/src/mcp/resources.rs` that serves it. This was already
  found unreachable in the M9 report (§19 item 49, point 1) since its only
  caller, `context.attach`, was removed in the first cut; now delete the
  dead code instead of leaving it documented as dormant.
- `subscriptions/listen` and everything in
  `crates/canvas-cli/src/mcp/subscribe.rs`, and any server-level wiring in
  `crates/canvas-cli/src/mcp/server.rs` that declares or dispatches it.
- Anything else in `mcp/` that exists only to serve the 22-tool catalog or
  the resource/subscription machinery (per-tool schemas, the effect
  annotation system if nothing else uses it, the `_meta` cursor/ttl
  handling if it only served resources). Use your judgment on what's
  genuinely dead versus still load-bearing for `getclitools` itself or for
  the MCP protocol handshake (`initialize`, `tools/list`, `tools/call`),
  and say which in your report.

**This still does not touch the CLI.** Every `canvas` command, its tests,
its registry entries, and `canvas-core` are unaffected. `extension/` is
unaffected. Only `crates/canvas-cli/src/mcp/`, the MCP schema wiring in
`output/json_schema.rs` if any is now dead, `tests/mcp.rs`,
`skill/canvas-cli/SKILL.md` and its workflow files, `docs/agent-hosts.md`,
`docs/SPEC.md` §19/§21, and `docs/writes-v2.md`/`docs/reads-v2.md` pointers
are in scope.

## What `getclitools`'s response should contain

The CLI already has a self-description mechanism built for exactly this:
`canvas schema --list` and `canvas schema <command>` (§21.1). Build the one
tool's handler on top of that existing machinery rather than writing new
documentation content by hand — call into whatever function backs
`canvas schema --list` to enumerate every command, and whatever backs
`canvas schema <command>` for each one's detail, and assemble the whole
thing into the tool's result. If `canvas schema` cannot currently describe a
command that agents actually need to run (a write command, `bridge`, etc.),
extend what it covers rather than inventing a second, parallel description
format — `canvas schema` and `getclitools` should describe the same CLI the
same way, one for a human/CLI caller, one for an MCP caller. Say in your
report whether you extended `canvas schema`'s coverage and why.

## What to rewrite

1. `crates/canvas-cli/src/mcp/catalog.rs`: replace the 22-tool `specs()`
   with the single `getclitools` `ToolSpec`. Delete every dispatch arm,
   every argument struct, and every schema constant that served a removed
   tool.
2. Delete `crates/canvas-cli/src/mcp/resources.rs` and
   `crates/canvas-cli/src/mcp/subscribe.rs` (or gut them to nothing, if the
   module structure needs to stay for another reason — explain if so).
   Remove their wiring from `server.rs` and anywhere `initialize`/
   `server/discover` advertised resources or subscription capability.
3. `tests/mcp.rs`: replace the catalog allowlist test and the invariant
   test from the M9 round with ones that assert there is exactly one tool,
   named `getclitools`, and that its result actually enumerates every
   `canvas` command (spot-check a handful by name in the response, don't
   just check it's non-empty). Remove every test that exercised a resource,
   a subscription, or a since-removed tool. Confirm `tools/list`'s response
   size (bytes, tokens) — it should be tiny now; measure it, don't guess.
4. `skill/canvas-cli/SKILL.md` and its workflow files: every one of the six
   workflows should now describe running `canvas` commands directly, for
   both reads and writes — the M9 round already did this for writes; extend
   the same treatment to reads. An MCP-connected agent's only MCP-specific
   step is calling `getclitools` once at the start.
5. `docs/agent-hosts.md`: rewrite to describe the one-tool surface; drop
   anything about resources, subscriptions, or per-tool annotations.
6. `docs/SPEC.md`:
   - §21.2 shrinks to describing one tool. §21's whole framing changes: this
     is no longer "a curated catalog of agent-facing actions", it is "a
     discovery tool for a CLI the agent runs itself." Rewrite the section
     header prose accordingly. Remove the resource and subscription
     subsections (currently under §21 and §22.5) or replace them with a
     one-line note that they do not exist, with the reason.
   - §22 (coordinator/events/watch/notify): the CLI's own `canvas watch` and
     `canvas notify` commands are UNCHANGED and stay fully documented — only
     the MCP subscription path into the same event log is gone. Say this
     explicitly so a reader does not think events themselves went away.
   - §19: add a new item recording this second, final decision (quote the
     owner's own words as above) and mark item 48 and item 49 each with a
     one-line "superseded by <this package's commit> — see item <N>" note;
     do not delete their text.
7. `docs/writes-v2.md` and `docs/reads-v2.md`: re-check both pointers are
   still accurate; they should now say MCP exposes no per-action tool at
   all, reads or writes, only the one discovery tool.
8. README: check for any MCP tool-count or tool-name claim and correct it.

## Gates

`cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D
warnings`, `cargo nextest run --all-features` (report the exact number and
account for the change from the M9 round's 919, the same way that round
accounted for 923→919), `cargo deny check`, `cargo +1.88 check --workspace
--all-targets`, `(cd extension && npm test)` (unaffected), `cargo xtask
bench --runs 3 --mcp` (regenerates `docs/bench.md`'s MCP section with the
new, much smaller numbers).

Commit as you go, small commits, prefix `feat(mcp)!:` or `docs(...)` as
appropriate. Never push. Never merge into `main`. `git merge main` first if
`main` has moved (it currently has not, relative to this lane's base).

Write your full final report, then reply exactly: `DONE M9b-mcp-single-tool`
