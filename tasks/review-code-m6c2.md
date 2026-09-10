# Code review + fix — M6-c2 on branch lane/w3 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M6-c2
on branch `lane/w3` (worktree `/home/user/projects/canvas-cli/.worktrees/w3`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /home/user/projects/canvas-cli/.worktrees/w3`. Read the package brief `tasks/m6c-coordinator-watch.md` and the SPEC sections it
   cites. First run `git merge main` (main gained M8-a and M8-a2 since the lane last merged; resolve if needed, keep both sides' entries, regenerate snapshots rather than hand-editing, and commit). Then read `git log main..HEAD --stat` and the full diff. Scope: M6-c itself was already reviewed (`docs/reviews/code-M6-c.md`, MERGE, five `review(M6-c):` fixes on this branch); this review covers only the follow-on M6-c2 on top of it: the merge of `main` (M6-b) into the lane at `6f8f7a7` and `f948e09` (check that both lanes' command enum arms, registry entries `schema@1`, `event@1`, `watch@1`, the submit plan flow plus the interest registration, both bench subcommands `--mcp` and `--watch`, and both `docs/bench.md` sections survived, with nothing lost or duplicated and no hand-edited snapshot), and brief item 7 (`canvas mcp` `subscriptions/listen` on the event log: an event on a scope invalidates the matching `canvas://<identity-key>/<generation>/…` resource and the server sends the resource-updated notification; a client's cursor follows the M6-c replay rules; nothing attaches or leaks across consumers or generations). Attack in particular: can a subscription ever deliver another identity generation's resource or a `not_attached` context resource's content? Does a `resync_required` on the cursor close the subscription honestly rather than pretending continuity? Is the notification driven by the durable event log (cursor-deduplicated, at-least-once) and never by an in-memory guess? Does the server still hold exactly one identity generation and stop cleanly on identity replacement? Does the interest registration in `submit`/`plan::execute` still happen before the first pre-flight request after the merge? Rerun `cargo xtask bench --runs 3`, `--mcp --runs 3`, and `--watch --runs 3` and confirm every §13 target. Write the verdict to `docs/reviews/code-M6-c2.md`. The worker's final report, for reference:

```
# M6-c2 — lane w3 final report

Worktree `/home/user/projects/canvas-cli/.worktrees/w3`, branch `lane/w3`,
`CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/w3`.
Nothing was pushed. Nothing was merged into `main`.

Commits added this round:

| Commit | Subject |
|---|---|
| `6f8f7a7` | `Merge branch 'main' into lane/w3` (the seven-file conflict resolution) |
| `605734d` | `feat(M6-c): make canvas mcp subscriptions/listen real on the event log` |
| `f948e09` | `Merge branch 'main' into lane/w3` (second merge, before reporting) |
| `27dcb14` | `chore(M6-c): record the bench run that measures both new surfaces` |

---

## 1. Task 1 — the merge

`git merge main` conflicted in seven files. Every conflict was resolved
keeping **both** lanes' additions. No existing entry was renamed or reordered.

| File | How it was resolved | What both lanes keep |
|---|---|---|
| `crates/canvas-cli/src/cli.rs` | By hand, both sides | `has_raw_output` keeps `Completions`, `Notify` (mine), `Schema` (w2), `Auth token --reveal`, `Config edit`. The `Watch`, `Notify`, `Schema`, and `Mcp` command arms all survive. `Cli::validate` keeps the `--json` refusal on `watch`. |
| `crates/canvas-cli/src/commands/submit.rs` | Took main's restructured plan flow, re-applied my addition | M6-a/M6-b freeze and plan flow, plus the foreground `Interest` registered before the pre-flight read and held for the whole command. Both destructuring sites hold it as `interest: _interest`. |
| `crates/canvas-cli/src/output/mod.rs` | By hand, both sides | My `EventJson`, `WatchResult`, `SCHEMA_EVENT`, `SCHEMA_WATCH` next to w2's `entry_for_command`, `entry_for_schema`, and the `json_schema` re-exports. |
| `crates/canvas-cli/src/output/registry.rs` | By hand, both sides | `schema@1` + `event@1` + `watch@1`. My two result types gained w2's `schemars::JsonSchema` derive, so `canvas schema` describes them. |
| `xtask/src/bench.rs` | Took main's file, re-applied `--watch` on top | Both `--mcp` and `--watch`. `Load::{Idle, Download, Watch}`, the `Tick` measurement, the watch harness, and w2's agent-surface measurement. |
| `xtask/src/main.rs` | By hand, both sides | Both clap flags, `Options { fixture, runs, no_fail, mcp, watch, doc }`. |
| `docs/bench.md` | Regenerated, never hand-edited | Both the `## Watch` section and the `## Agent surface (canvas mcp)` section, from one run. |

The registry envelope snapshot
(`crates/canvas-cli/src/output/snapshots/canvas__output__registry__tests__registered_envelopes.snap`)
was **regenerated** with `INSTA_FORCE_UPDATE=1`, not edited. It holds 45
`canvas-cli/` entries, including `canvas-cli/watch@1` and `canvas-cli/event@1`.

Faults found and fixed while porting my commands onto M6-b's handler
extraction: the deleted `commands::emit::emit`, the changed `emit_error` and
`session_error` signatures and return type, stale `globals` arguments left in
`watch`'s internal calls, and `db_error` still returning `ExitCode` instead of
`Handled`.

A second `git merge main` before reporting brought only `docs/` and `tasks/`
files and needed no resolution. A third merge reports "Already up to date".

---

## 2. Task 2 — item 7: `subscriptions/listen` on the event log

New module `crates/canvas-cli/src/mcp/subscribe.rs`, wired into
`crates/canvas-cli/src/mcp/server.rs`.

### Invalidation

An event on a dataset scope invalidates the resources that read that scope.
The server sends one `notifications/resources/updated` per resource, and one
batch invalidates a resource once.

| Event dataset and scope | Resources invalidated |
|---|---|
| `assignments`, scope `course:<id>` | `canvas://<key>/<generation>/course/<id>/assignments` and `.../todo` |
| `missing` | `.../todo` |
| `submission_journal` | `.../receipts` |
| anything else | nothing |

### Namespace

`accepted_subscription_filter` keeps only the URIs this instance serves.
A foreign identity key, another identity generation, and an unknown path all
address nothing here, so they are dropped from the accepted filter instead of
refused: a host is told exactly which of its names can be updated. Server
capabilities now advertise `resources.subscribe`; without it the SDK strips
the resource filter before the acknowledgment.

### Replay rules (the `watch --since` rules, §3.2 and §3.6)

1. The position is durable per consumer, under the consumer name the tool
   calls already use (`mcp` or `mcp:<host>`), in the same `consumer_cursor`
   table `notify` uses.
2. A host may name its own position in `_meta` under `dev.canvas-cli/cursor`.
   A decimal string is the §7 form; a JSON number is accepted too. A negative
   or unparsable value is ignored.
3. `check_cursor` classifies the position. `Replay` replays every row after it,
   in cursor order, at least once.
4. `Resync` (expired, or from another identity generation) invalidates every
   subscribed resource once, then follows the log from its high water mark. The
   host rebuilds its own baseline by reading again. The gap is reported, never
   hidden.
5. The position moves only after the notifications are sent, so a stream that
   ends mid-batch replays that batch instead of dropping it.
6. The subscription holds the shared identity lock for its life, so a
   subscribed host is a resident consumer and `identity remove` reports busy
   (§3.4).

### One correctness fix outside item 7

`events::set_consumer_cursor` never moves a position back, which is right for
progress but wrong for a resync: a stored cursor the log can no longer replay
made `notify` report the same gap on every run and post nothing ever again.
New `canvas_core::events::reset_consumer_cursor` replaces such a position.
Both the subscription and `notify` use it. Both files are ones this lane owns.

### Other notes

- The rmcp SDK answers the **first** request of a connection inline, before
  its service loop starts. A connection that opens a subscription as its very
  first request therefore gets no other answer while the stream is open. A
  host discovers the server first, which is what the tests do; the limitation
  is documented in the module.
- One debug-only test variable was added: `CANVAS_TEST_MCP_POLL_MS`.
- Skill parity: `skill/canvas-cli/SKILL.md` documents the subscription in its
  Resources section — how to subscribe, what each event invalidates, the
  acknowledgment, the durable position, the `_meta` cursor, and the resync.
- Catalog parity: the tool catalog needed no change. No tool was added,
  removed, or renamed.

---

## 3. Readings chosen where the report leaves something undefined

Each choice takes the reading that emits fewer events and never claims a
change that was not observed.

1. `announcements` has an event kind but no resource, so an announcement event
   invalidates nothing.
2. `submission_journal` invalidates `receipts` only, not `todo`. A journal
   transition is local; nothing about the deadline set was observed on Canvas.
3. `assignments` invalidates `todo` as well as that course's resource, because
   the todo window renders the assignment rows the planner names.
4. `courses`, `planner`, and the grade datasets produce no events at all, so
   they invalidate nothing.
5. A row from another identity key or generation invalidates nothing, even
   while it sits in the log.
6. `context/<handle>` stays subscribable, because it is a name this server
   serves, but no event ever names it.
7. A resync invalidates every subscribed resource once, including resources no
   event named: after a gap, nothing the host holds is known to be current.
8. A subscription that asks only for the resource-list category stays quiet, as
   before: one instance serves one generation, so its resource list is fixed.

---

## 4. Tests added

Unit tests:

- `mcp::subscribe`: the dataset-to-resource mapping; the identity and
  generation guard; one notification per resource per batch and none for an
  unsubscribed resource; the accepted filter narrowing (unknown path, second
  generation, foreign identity, foreign scheme); the `_meta` cursor parsing.
- `mcp::server`: a list-only subscription still promises no resource events;
  a subscription keeps only the URIs the instance serves, and the capability
  advertises `subscribe`.
- `canvas-core::events`: a consumer position moves forward only, until a
  resync replaces it, and consumers are independent.

End-to-end tests over a real stdio pipe (`crates/canvas-cli/tests/mcp.rs`):

- `an_event_invalidates_the_matching_resource_and_no_other` — the
  acknowledgment content and its subscription id, the notification and its
  subscription id, and the log position moving past events that name no
  subscribed resource.
- `a_cursor_the_log_cannot_replay_asks_for_a_resync` — every subscribed
  resource is invalidated once, and the stored position is replaced so the
  same gap is not reported again.
- `a_host_resumes_from_the_cursor_it_names` — no cursor replays the whole log;
  `dev.canvas-cli/cursor` `1` replays only the row after it.

`crates/canvas-cli/tests/e2e/m6c.rs`: `notify`'s stale stored position asks
for a resync once, and the next run posts again.

---

## 5. Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | ok |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo nextest run --all-features` | **695 tests run, 695 passed, 0 skipped** |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | ok |
| `cargo xtask bench --runs 3` | every §13 target ok |
| `cargo xtask bench --watch --runs 3` | every §13 target ok; watch tick p50 22.3 ms, p95 22.5 ms |

`docs/bench.md` was regenerated last with
`cargo xtask bench --mcp --watch --runs 3`, so it carries the `## Watch`
section and the `## Agent surface (canvas mcp)` section from one run. Its
header now names both flags, so the page says how to reproduce itself.

`git status --short` is empty.
```
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/rev-m6c2`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M6-c2):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M6-c2.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M6-c2.md`
