# Code review + fix — M8-a3 on branch lane/w3 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M8-a3
on branch `lane/w3` (worktree `/Users/rolfie/projects/canvas-cli/.worktrees/w3`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /Users/rolfie/projects/canvas-cli/.worktrees/w3`. Read the package brief `tasks/m8a-richer-reads.md` and the SPEC sections it
   cites. First run `git merge main` (expect nothing to do; resolve if needed and commit). Then read `git log main..HEAD --stat` and the full diff. Scope: this is the small follow-on M8-a3 on top of already-merged work: item 4 of the M8-a brief (the `inbox.unread_count` event produced from the `inbox_unread` dataset under M6-c's baseline rules) and two schema-page observations from `docs/reviews/code-M6-c2.md` (`canvas schema event` describes the `--jsonl` line without claiming a §7 envelope; `canvas schema watch` says `--json` is refused and `--jsonl` is the machine form). Contract: `docs/agent-ux/REPORT.md` §3.6 (first observation silent, complete same-scope observations only, allowlisted payload, cursor dedup, no silent gap), `docs/reads-v2.md`, `docs/SPEC.md` §7, §15. Attack in particular: does the first observation stay silent and does an unchanged count emit nothing; is the event produced from the durable observation outbox (a kill between the cache commit and the state transaction replays or emits `resync_required`, never a phantom or a lost event); is the payload only the two counts and the identity/scope fields (no message, no subject, no sender); does the `inbox unread-count` command's refresh go through the same observation path as the `watch` tick without double-emitting; is `null`/unparseable count never turned into a `0`; do the schema pages match the actual output. Rerun `cargo xtask bench --runs 3 --mcp --watch` and confirm every §13 target. Write the verdict to `docs/reviews/code-M8-a3.md`. The worker's final report, for reference:

```
# M8-a3 — final report (lane `w3`)

Worktree `/Users/rolfie/projects/canvas-cli/.worktrees/w3`, branch `lane/w3`,
`CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/w3`.

Three commits, plus the merge of `main`. Nothing was pushed and nothing was
merged into `main`.

| Commit | What |
|---|---|
| `f438755` | `feat(M8-a3): produce inbox.unread_count from the inbox_unread dataset` |
| `dae6758` | `fix(M8-a3): describe the jsonl stream contract in canvas schema` |
| `8c6133c` | `chore(M8-a3): re-record docs/bench.md from the gate runs` |
| `d7b3fa5` | `Merge branch 'main' into lane/w3` |

## Item 1 — the `inbox.unread_count` event

### Why one shape was enough

`refresh_dataset` already calls `crate::events::observe_refresh(store,
dataset.name(), dataset.scope_key())` after every successful cache commit, and
`commit_refresh` writes the `membership` row generically. The observation path
is therefore already shared by every dataset. Item 4 needed no second
implementation and no producer of its own: it needed a `Shape`.

That is what makes the brief's "emitted by the watch tick and by the inbox
unread-count command's refresh through the same observation path" true by
construction rather than by two code paths that must be kept in step.

### The shape

`crates/canvas-core/src/events/compare.rs`:

```rust
Shape {
    // The unread count is one row (M8-a `inbox_unread` / `all`). The
    // payload is the count and nothing else: a conversation subject, a
    // participant, and a message body never reach the log.
    dataset: "inbox_unread",
    table: "conversation_unread",
    columns: &["unread_count"],
    json_keys: &[],
    // The first complete observation sets the baseline and says nothing,
    // so `added` can only follow a gap that dropped that baseline. The
    // count became known again, which is the same news as a change.
    added: EventKind::InboxUnreadCount,
    removed: None,
    changed: Some(EventKind::InboxUnreadCount),
},
```

One supporting change was needed. `Shape.compare_fields: bool` became
`Shape.changed: Option<EventKind>`, so a shape names its own change kind
instead of inheriting a hardcoded one:

- `diff` now calls
  `if let Some(changed) = shape.changed { events.extend(field_events(changed, key, before, after)); }`
- `field_events(changed_kind, key, before, after)` uses `changed_kind` for the
  "rest" bucket instead of the hardcoded `EventKind::AssignmentChanged`.
- `assignments` sets `changed: Some(EventKind::AssignmentChanged)`; `missing`
  and `announcements` set `changed: None`. Behaviour of all three is unchanged.

`crates/canvas-core/src/events/kind.rs`: the `InboxUnreadCount` doc comment
changed from "Registered for M8-a; this package writes no producer for it." to
"The unread-conversation count changed (M8-a `inbox_unread`)."

### The baseline rules it inherits

Nothing in the M6-c protocol was relaxed for this dataset:

- The first complete observation sets the baseline and emits nothing.
- Only a **complete** observation is compared: the coverage row must be
  `complete=1, stale=0`, and only against the baseline of the **same** dataset
  and scope.
- The payload is the allowlist and nothing more: `unread_count` before and
  after. A conversation subject, a participant, and a message body cannot
  reach the log because they are not in `columns` and `json_keys` is empty.
- An observation is identified by `<dataset>:<scope>:<fetch_log rowid>:<fetched_at>`
  and is applied once. Re-applying an `applied` row emits nothing.
- A vanished or overwritten cache row still emits one `resync_required`.

### The producers

- **The watch tick** — `crates/canvas-cli/src/commands/watch.rs` refreshes
  `inbox_unread` / `all` last:

  ```rust
  // The unread count is last. It is the cheapest dataset and the least
  // urgent one, so a slow inbox never delays the coursework a deadline
  // depends on, and a failing one backs off on its own (M8-a `inbox_unread`,
  // REPORT §3.6 `inbox.unread_count`).
  ```

  It goes through the same `attempt` / `record` wrapper as every other dataset,
  so it takes part in the tick's backoff and appears in the tick summary.

- **`canvas inbox unread-count`** — unchanged. Its refresh already runs through
  `refresh_dataset`, so it observes on the same path.

### The four readings I chose

The report is silent on these; `docs/reads-v2.md` now records each one and why.

1. **A gap that dropped the baseline emits `InboxUnreadCount`, not a distinct
   "added" kind.** The first complete observation is silent, so `added` can only
   fire after a gap removed the baseline. At that moment the count became known
   again, which is the same news to a consumer as a change. Inventing a second
   kind would make every consumer handle two kinds that mean one thing.
2. **There is no removal kind.** `removed: None`. The unread count is one row
   that is either known or not known. "The count went away" is a coverage fact,
   and coverage already has its own signal in `resync_required`.
3. **The count is refreshed last in a tick.** It is the cheapest dataset and
   the least urgent. Putting it last means a slow or failing inbox never delays
   the coursework a deadline depends on.
4. **A `null` count compares like any other value.** No special case. If the
   count is absent on one side, that is a change, and the payload says so.

### Tests

`crates/canvas-core/src/events/tests.rs` — new helpers `seed_unread(store,
count, fetched_at)` (writes `conversation_unread`, the `membership` row
`('inbox_unread','all','inbox_unread','1',0)`, and complete `fetch_log`
coverage), `payloads(store)`, `row_id(store)`:

- `the_unread_count_reports_only_what_changed` — the first observation is
  silent; an unchanged count emits nothing; a changed count emits exactly one
  event with `before {"unread_count":3}` and `after {"unread_count":5}`.
- `an_unread_count_observation_is_applied_once` — record, apply twice,
  re-observe; exactly one event (replay dedup).

`crates/canvas-cli/tests/e2e/m6c.rs`:

- `a_changed_unread_count_is_one_event_from_the_watch_tick` — the tick does
  refresh `inbox_unread`; an unchanged count says nothing; a changed count is
  exactly one event with `dataset: "inbox_unread"`, `scope: "all"`,
  `before {"unread_count":2}`, `after {"unread_count":5}`.
- `the_unread_count_command_records_its_own_observation` — two
  `inbox unread-count` runs at different `CANVAS_NOW`, then `notify --stdout`
  prints a line starting `inbox:`.

`crates/canvas-cli/tests/e2e/harness.rs` — `mount_inbox_unread` serves
`/api/v1/conversations/unread_count`, called from `CanvasServer::start`.

Two snapshots (`m6c_watch_once.snap`, `m6c_watch_stream.snap`) were regenerated
with `INSTA_FORCE_UPDATE=1`. Both gained the `inbox_unread` / `all` dataset row
and `requests.api` 6 → 7. The diffs were read to confirm nothing else moved.

Two traps found while writing these tests, both fixed in the tests themselves:

- `NO_TTL` did not set `ttl_inbox`, so the 5-minute default kept serving the
  cache and no refresh happened. Added `ttl_inbox = "0m"`.
- Two runs at the same `CANVAS_NOW` produce the same observation id, so the
  second is correctly a no-op. Each observing run now has its own instant
  (`LATEST` for the third tick, `LATER` for the second `inbox unread-count`).

### Documentation

`docs/reads-v2.md` — the "Left for the next round" note was replaced by the
section "The `inbox.unread_count` event (item 4, done in M8-a3)", covering the
shape, the baseline rules, both producers, and the four readings above. The
file now ends with "## Left for the next round — Nothing from this package's
brief remains."

## Item 2 — the two review observations from `docs/reviews/code-M6-c2.md`

The review's observation: `canvas schema watch` and `canvas schema event`
rendered the uniform `schema@1` page, which always carries an `envelope` and an
`error` section. For `event@1` that is generous — a `--jsonl` line is
self-describing and never wrapped — and for `watch@1` the page never said that
`--json` is refused and `--jsonl` is the machine-readable form.

`crates/canvas-cli/src/output/json_schema.rs` now derives a form from the
schema id and always writes an `output` block:

```rust
enum Form { Envelope, StreamSummary, StreamLine }

fn form_of(schema_id: &str) -> Form {
    match schema_id {
        SCHEMA_EVENT => Form::StreamLine,
        SCHEMA_WATCH => Form::StreamSummary,
        _ => Form::Envelope,
    }
}
```

with `output_section(form)` (`$comment`, `form`, `flag`, `printed_by`,
`refuses`), `pin_schema_field` and `line_schema`.

- `canvas schema event` — `form: "jsonl"`, `flag: "--jsonl"`,
  `refuses: ["--json"]`. The document renders `line` **only**: no `envelope`,
  no `result`, no `error`. The line's `schema` field is pinned with a `const`
  of `event@1`.
- `canvas schema watch` — `form: "envelope"`, `flag: "--jsonl"`,
  `refuses: ["--json"]`, with a `$comment` naming both, and the `error@1`
  branch kept.
- Every other schema keeps the envelope form byte-for-byte, plus the new
  `output` block.

`result_schema` gained `SCHEMA_EVENT => Some(schema_of::<EventJson>())` and
`SCHEMA_WATCH => Some(schema_of::<WatchResult>())`.

**Kept minimal on purpose.** `SchemaEntry` is w2's type with roughly 53
constructors and is a shared file this lane does not own this round. It was not
changed: the form is derived from the schema id inside `json_schema.rs`.

Tests:

- `the_event_schema_describes_a_line_and_claims_no_envelope` (unit)
- `the_watch_schema_says_json_is_refused` (unit)
- `every_registered_schema_has_a_document` now branches on the form
- `the_stream_schemas_describe_the_jsonl_contract` in
  `crates/canvas-cli/tests/schema_cmd.rs` — asserts both pages end to end

One snapshot (`canvas__output__json_schema__tests__calendar_schema_document.snap`)
was regenerated; it gained the `output` block and nothing else.

Two small things found while writing this:

- `schemars` renders a `serde_json::Value` field as the always-true schema
  `true`, so `document["line"]["properties"]["before"].is_object()` fails. The
  test asserts presence with `.get(field).is_some()` instead.
- A Rust line-continuation inside a `$comment` literal kept its indentation and
  produced long runs of spaces in the output. The three new `$comment` literals
  are single-line strings. The pre-existing `envelope.$comment` was left alone,
  because changing it would churn every MCP tool `outputSchema`.

## Gates

All seven gates from `tasks/m6c-coordinator-watch.md`.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo nextest run --all-features` | **730 run, 730 passed, 0 skipped** |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | ok (run in `.target/w3-188`) |
| `cargo xtask bench --runs 3` | every §13 target ok |
| `cargo xtask bench --watch --runs 3` | every §13 target ok; tick p50 21.7 / p95 23.2 ms |

A final `cargo xtask bench --mcp --watch --runs 3` recorded the page in one
run, so `docs/bench.md` carries both the `## Watch` section and the
`## Agent surface (canvas mcp)` section together:

```
cached todo, first output    idle             7.0       7.1          150      ok
cached todo, full run        idle             7.7       7.8          250      ok
cold start                   idle             9.9      10.6          400      ok
cached todo, first output    download         6.9       7.1          150      ok
cached todo, full run        download         7.5       7.6          250      ok
cold start                   download        11.3      11.3          400      ok
cached todo, first output    watch            6.8       6.9          150      ok
cached todo, full run        watch            7.3       7.5          250      ok
cold start                   watch           11.3      11.4          400      ok
watch tick                   idle            21.3      21.7         none      —
warm todo.list over stdio    mcp              2.6       2.7          100      ok

catalog: 30 tools, 220855 bytes, ~55224 tokens per `tools/list`
```

Test count: 723 at the M6-c2 merge, 730 now — seven added by this package.

## Files changed

| File | Why |
|---|---|
| `crates/canvas-core/src/events/compare.rs` | the `inbox_unread` shape; `compare_fields` → `changed: Option<EventKind>` |
| `crates/canvas-core/src/events/kind.rs` | `InboxUnreadCount` now has a producer |
| `crates/canvas-core/src/events/tests.rs` | `seed_unread`, `payloads`, `row_id`, two tests |
| `crates/canvas-cli/src/commands/watch.rs` | the tick refreshes `inbox_unread` last |
| `crates/canvas-cli/src/output/json_schema.rs` | `Form`, `output_section`, `line_schema`, `pin_schema_field`, two tests |
| `crates/canvas-cli/tests/e2e/harness.rs` | `mount_inbox_unread` |
| `crates/canvas-cli/tests/e2e/m6c.rs` | `ttl_inbox = "0m"`, `LATEST`, two tests |
| `crates/canvas-cli/tests/schema_cmd.rs` | `the_stream_schemas_describe_the_jsonl_contract` |
| three `.snap` files | regenerated, never hand-edited |
| `docs/reads-v2.md` | the permitted docs edit; item 4 recorded, note retired |
| `docs/bench.md` | rewritten by the bench gates themselves |

## Merge with `main`

`git merge main` brought one docs-only change: `docs/SPEC.md`, 4 insertions,
1 deletion (Appendix A tokio features, §19 items 30-32 from the M7-a review).
No code moved, so the gate results above still hold. The working tree is clean.
```
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m8a3`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M8-a3):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M8-a3.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M8-a3.md`
