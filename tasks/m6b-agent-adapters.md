# M6-b — Agent adapters: `canvas mcp`, `canvas schema`, the shipped skill (Claude Opus, lane w2)

Post-v1 package from the agent-UX design. Read `docs/agent-ux/REPORT.md`
§1, §3.1, §3.2 (**all of it**: the surface table, the schema registry
additions, the CLI exit mappings, the catalog exclusions, annotations,
results, resources, the protocol target and the host matrix), §3.5 (the
approval handle and how `execute` returns `input_required`), §4 (the M6-b
row and its acceptance column), §5 (host reachability), and the sources
S8–S11, S27, S28 it cites (read the MCP 2026-07-28 tools, elicitation,
multi-round-trip, and 2025-11-25 lifecycle pages, and the `rmcp` 3.2.0
docs; pin `rmcp = "=3.2.0"` and record the exact protocol features you
rely on). Then `docs/SPEC.md` §5, §7, §13, §14, §15, Appendix D. Existing
code: `canvas-core::plan` (M6-a: `prepare`, `issue_handle`, `approve`,
`execute`), every command module and renderer, `crates/canvas-cli/src/
output` (envelope, registry). Read their public APIs first. Precedence:
REPORT §3.2 and §3.5 define the adapter; SPEC §7 and §14 define what an
envelope and an exit code mean; the CLI's `--json` output is the contract
the adapter must reproduce, never a second implementation of a command.

Not in this package: `context.*`, `here`, `bridge`, `follow` (M7),
`watch`/events (M6-c), any remote or HTTP transport.

## Deliverables
1. **Reusable command handlers.** Extract each v1 command's core into a
   function that takes typed arguments and returns the §7 envelope (the
   same struct the `--json` renderer serializes) so the CLI and the MCP
   adapter share one implementation. No shelling out to `canvas`.
2. **`canvas schema <command>`** (raw output like `completions`; schema
   `schema@1`; `--json` is exit 2): prints the JSON Schema of that
   command's `--json` envelope and `result`, generated from the registry
   fixtures and types (not hand-written), including the domain-error shape.
   `canvas schema --list` prints the registry.
3. **`canvas mcp`** (stdio, `rmcp` 3.2.0): startup binds one identity key
   and generation from the selected profile (§8 selection matrix; no
   identity → refuse to start with the §14 auth error); identity replacement
   or removal stops the instance cleanly. Protocol `2026-07-28` primary
   (`server/discover`, per-request version and capabilities, `input_required`
   elicitation with `requestState` and keyed `inputResponses`,
   `subscriptions/listen` declared but with no resources that change in
   this package) and `2025-11-25` through an explicit adapter; any other
   version fails the handshake with a clear message.
   - **Tool catalog exactly as REPORT §3.2**: `courses.list`, `course.get`,
     `todo.list`, `assignments.list`, `assignment.get`, `grades.get`,
     `files.list`, `modules.list`, `announcements.list`, `announcement.get`,
     `calendar.list`, `submission.get`, `receipts.list`, `receipts.show`,
     `sync.run`, `download.plan`, `download.run` (configured destination
     only; no `--force`), `submission.prepare`, `submission.execute`,
     `submission.reconcile` (`assume_not_submitted` defaults false and is
     the only argument that retires evidence), `receipts.acknowledge`,
     `open.url` (resolves and returns the URL; never launches). Each
     preserves the v1 arguments and the §7 result. **Absent by design**:
     credentials, token reveal, identity administration, arbitrary HTTP or
     shell, `--yes`, cache clearing, `download --force`, any browser action.
   - Annotations describe effects (`readOnlyHint` true only for pure
     reads; `submission.prepare` false; `destructiveHint` never true;
     `idempotentHint` where true). Hints are documentation, not
     enforcement: enforcement is the plan/approval core.
   - **Results**: `structuredContent` = the full §7 envelope; text content
     = the same JSON serialized; domain failures keep the envelope with
     `outcome`/`exit` (exit mapping table in §3.2, e.g. `approval_required`
     → outcome `refused`, exit 8); protocol and argument failures use MCP
     errors; every output schema admits both shapes. Private results carry
     `cacheScope: private` and `ttlMs` ≤ the remaining freshness of the
     oldest dataset in the envelope, 0 for anything unresolved.
   - **Approval flow**: `submission.execute(plan_id)` on an approved plan
     runs; on a prepared plan it returns `input_required` with
     `requestState = { plan_id, handle }` from `issue_handle`; the host's
     retry carries the handle in `inputResponses`; accept → `approve(channel
     "elicitation", consumer = the session's consumer id)` then execute;
     decline or cancel → invalidate; a replayed acceptance returns the
     existing journal. A host that declares no elicitation support gets a
     domain `approval_required` refusal with the handle and **nothing is
     dispatched**. Ordinary tool arguments can never assert approval.
   - **Resources** namespaced by identity **and generation**, canonical
     encoding: `canvas://<identity-key>/<generation>/todo`,
     `/course/<id>/assignments`, `/receipts`; `/context/<consumer-handle>`
     exists and returns `not_attached` (the bridge is M7). Reads go through
     the same handlers. Nothing leaks another consumer's data.
4. **The shipped skill** under `skill/canvas-cli/` (`SKILL.md` plus one
   file per workflow): what the tool is, the identity model, the five
   workflows (organize the week, read an assignment, prepare and submit
   with approval, reconcile an unknown outcome, download course files),
   the exit-code and recovery table, the `--json`/`canvas schema` contract,
   and the MCP setup for Claude Code (local stdio, scope, explicit profile),
   Codex, and Cursor. Included in the `cargo dist` archives (extend the
   M5-b config) and referenced from the README.
5. **Host matrix and measurements** in `docs/agent-hosts.md` (the one docs
   file you may write): for Claude Code, Codex, and Cursor, what you
   actually ran on this machine (a scripted non-interactive session where
   the host CLI allows it, e.g. `claude -p` with a project MCP config),
   which protocol revision was negotiated, whether `input_required` worked,
   and what stayed untested; never claim an untested host works. Add
   `cargo xtask bench --mcp`: schema token estimate per tool (a
   documented tokenizer approximation is acceptable if stated), warm
   `todo.list` round-trip p50/p95 over stdio (target p95 < 100 ms excluding
   model time), and tool calls per workflow for the five skill workflows;
   append the numbers to `docs/bench.md`.
6. **Tests**: every tool's success and domain-error envelope snapshot equals
   the CLI's `--json` output for the same fixture; both protocol versions
   handshake and an unknown one is refused; the catalog contains no
   forbidden tool and no forbidden argument (a test enumerates the catalog
   against an allowlist); accept, decline, cancel, and replay through
   `input_required`; the no-elicitation refusal dispatches nothing (wiremock
   sees no upload or POST); resources are private to the identity
   generation (a second generation sees nothing); `canvas schema` matches
   the registry and rejects `--json` with exit 2; the skill's command names
   match the catalog (a test diffs them).

## Rules
- You own `crates/canvas-cli/src/mcp/**`, `crates/canvas-cli/src/commands/
  schema.rs`, the handler extraction (keep each command's renderer where
  it is), `skill/**`, `docs/agent-hosts.md`, the `bench --mcp` extension,
  and the registry entry `schema@1`. Single-lane round for the enum,
  registry, and migration (none expected).
- Work on branch `lane/w2` in this worktree with
  `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/w2`. Commit
  as you go with conventional messages. Before reporting, `git merge main`
  (resolve, rerun gates). Do not push. Do not merge into `main`.
- Do not touch `docs/` (except `docs/agent-hosts.md` and the `bench.md`
  append) or `tasks/`. Where the report leaves something undefined, choose
  the reading that exposes less and never dispatches a remote write
  without a recorded human approval; name each choice in your final message.

## Gates
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
cargo xtask bench --mcp --runs 3
```
Finish with `git status --short` and reply with the marker `DONE M6-b` on
its own line.
