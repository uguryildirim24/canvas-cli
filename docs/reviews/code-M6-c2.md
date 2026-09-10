# Code review — M6-c2 (lane `w3`)

**MERGE.** The merge of `main` (M6-b, then M8-a/M8-a2) into the lane lost
nothing and duplicated nothing, and `subscriptions/listen` is driven by the
durable event log, not by a guess in memory.
Five defects were found and fixed on this branch: one that let a host subscribe
to another consumer's context resource, one that destroyed a consumer's stored
position when the *host* named an unreplayable cursor, two missing tests the
brief's attack list requires, and one comment the merge made untrue.
Every gate passes, and all four bench runs meet every §13 target.

Scope: only the follow-on on top of M6-c, which was reviewed separately in
`docs/reviews/code-M6-c.md`. That is `6f8f7a7`, `605734d`, `f948e09`,
`27dcb14`, and the third `main` merge this review had to do first (`a44bc45`).

## Gates

`CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m6c2`.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo nextest run --all-features` | **723 tests run, 723 passed, 0 skipped** |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | ok (MSRV holds; Appendix A pins unchanged) |
| `INSTA_FORCE_UPDATE=1 cargo nextest run --all-features` | no snapshot drift: **no file changed** |
| `cargo xtask bench --runs 3` | every §13 target ok |
| `cargo xtask bench --mcp --runs 3` | every §13 target ok; warm `todo.list` over stdio p95 2.9 ms (< 100) |
| `cargo xtask bench --watch --runs 3` | every §13 target ok under a resident `watch`; tick p50 26.4 ms |
| `cargo xtask bench --mcp --watch --runs 3` | every §13 target ok; the recorded page |

The `INSTA_FORCE_UPDATE` run is how this review checked the brief's
"no hand-edited snapshot" condition, including the four `doctor` snapshots this
merge had to resolve by hand and the 53-entry registry envelope snapshot. Every
byte on disk is what the code produces.

## The merge (`6f8f7a7`, `f948e09`, and this review's `a44bc45`)

Both lanes survive intact. Checked directly rather than from the report:

| Surface | State |
|---|---|
| `Commands` enum | 35 arms; w2's `Pages`, `Page`, `Syllabus`, `Discussions`, `Discussion`, `Inbox` and w3's `Watch`, `Notify` all present, plus `Schema` and `Mcp` |
| `Cli::has_raw_output` | keeps `Completions`, `Notify`, `Schema`, `Auth token --reveal`, `Config edit`, and both stdout redirections |
| `Cli::validate` | keeps the separate `--json` refusal on `watch` (exit 2, verified by hand) |
| Registry | `schema@1`, `event@1`, `watch@1` registered; `canvas schema watch` and `canvas schema event` both answer; both result types carry w2's `schemars::JsonSchema` derive |
| Registry snapshot | 53 `canvas-cli/` envelopes, no unintended duplicate id |
| Migrations | `CACHE_USER_VERSION = 2` (w2's `0002_reads`) and `STATE_USER_VERSION = 3` (w3's `0003_events`) both applied; the doc table names both, and the four `doctor` snapshots read `cache schema=2, state schema=3` |
| `submit` plan flow | M6-a/M6-b freeze and plan flow kept, with the foreground `Interest` held for the whole command through `Frozen`; both destructuring sites hold it |
| `xtask bench` | both `--mcp` and `--watch`; `Load::{Idle, Download, Watch}` and the agent-surface measurement all present |
| `docs/bench.md` | both `## Watch` and `## Agent surface (canvas mcp)`, regenerated from one run |

## Defects found and fixed

| # | Severity | Where | What was wrong | What changed | Commit |
|---|---|---|---|---|---|
| 1 | **High** | `crates/canvas-cli/src/mcp/resources.rs:50`, `crates/canvas-cli/src/mcp/subscribe.rs:80` | The subscription filter was narrowed with `Binding::serves`, which is true for `context/<consumer-handle>` too, so a host could subscribe to **another consumer's** context resource. Nothing ever invalidates it, so the promise `serves` documents — "a host never holds a subscription to a name that can never be invalidated" — was not kept; and because the resync branch invalidates *every* subscribed resource, such a host would be told that another consumer's context changed, which nothing observed. REPORT §3.2 rules this out twice: "Explicit consumer attachment; **no implicit sharing from resource subscriptions**", and "No tool/resource discovery leaks another consumer's text." | `Binding::serves` became `Binding::target`, which only says what a URI names; `subscribe::subscribable` decides which of those an event can reach — `todo`, `receipts`, `course/<id>/assignments`. A consumer context stays readable and is never subscribable. Unit test and the end-to-end acknowledgment test both assert it is dropped. | `43ddd79` |
| 2 | **Medium** | `crates/canvas-cli/src/mcp/subscribe.rs:225` | `listen` replaced the durable consumer position with the log's high water mark on **any** resync, including one caused by a cursor the *host* named in `_meta`. A named cursor is the host's own position and says nothing about the stored one: the stored one was still replayable and still named rows the consumer had never been told about. Stepping it forward dropped those rows for every later connection that names no cursor. `notify` already draws exactly this line — "an explicit `--since` is the caller's own position, and it never touches the stored one" — so the two consumers of one log disagreed. | The reset now happens only when the resync came from the stored position. The gap is still reported: every subscribed resource is still invalidated once. New end-to-end test `a_named_cursor_that_asks_for_a_resync_leaves_the_stored_position_alone`, confirmed to fail on the old code (stored position 2 instead of 1). | `095fde6` |
| 3 | **Medium** | `crates/canvas-cli/tests/mcp.rs` (missing test) | Item 7 asks whether the server still holds exactly one identity generation and stops cleanly when it is replaced. A subscription now holds an identity session open for the whole life of the stream — that is what makes a subscribed host a resident consumer under §3.4 — and nothing measured that this hold does not also keep a replaced instance alive. `replacing_the_identity_stops_the_instance` only covers an instance with no stream open. | `a_live_subscription_does_not_keep_a_replaced_identity_alive` establishes a stream, sees a notification arrive on it, writes a new generation to `identity.json`, and asserts the instance exits 13. It passes on the current code. | `28ee2e8` |
| 4 | Low | `crates/canvas-cli/src/mcp/subscribe.rs:112` (missing test + wrong doc) | `observe::report_gap` writes a `resync_required` row carrying the dataset and scope whose observation was lost, and `invalidated` keys on the dataset scope rather than the kind — so a gap already invalidates the resources that read that scope. That is right, but the module said the opposite ("a URI is invalidated only when a row says its scope changed") and nothing measured it, so keying the mapping on kind later would have silently stopped reporting gaps to subscribed hosts. | The doc states both reasons a row invalidates a scope; `a_gap_on_a_scope_invalidates_the_resources_that_read_it` holds the behaviour. | `50fba0f` |
| 5 | Low | `crates/canvas-cli/src/commands/submit.rs:179` | The merge rewrote the comment above `register_interest` to claim the registration happens "before the pre-flight read of the assignment". It does not: `session.validate_network_token()` and `resolve_target` — the code's own "Pre-flight step 1" — both run first, and both make requests. A comment asserting an ordering the code does not have would let the next reader take REPORT §3.6's "before its first preflight request" as already satisfied. | The comment now says when the registration really happens, why the ordering is forced (an interest is keyed by assignment, and `resolve_target` is what produces the id), and which part of the command it does cover. See "Needs a decision". | `b857cd9` |

`c7d5adc` re-records `docs/bench.md` after the fixes.

## What the attack list found clean

- **Another generation's resource, or a `not_attached` context resource's content.** Two independent guards, and now a third. `invalidated` drops any row whose `identity_key` *or* `generation` differs from the binding; the notified URI must also be in the accepted filter, every entry of which carries this binding's key and generation; and content never travels on a notification at all — the host re-reads through `resources::read`, which refuses a foreign URI with `resource_not_found` and answers `context/<handle>` with a §7 `refused` / `not_attached` envelope carrying no text. After defect 1, a context resource cannot even be named on a subscription.
- **`resync_required` closing honestly.** The MCP notification vocabulary has no "resync" message, so the stream invalidates every subscribed resource once and continues from the log's high water mark. That is the honest translation: the host is told that nothing it holds is known to be current, which is what the §3.2 exit row means, and the gap is never papered over. It is not silently swallowed either — the durable position is replaced so the same gap is reported once, and (after defect 2) only when the gap was the stored position's.
- **Durable, cursor-deduplicated, at-least-once.** Every notification comes from `events::read_after` against `state.sqlite`. The position advances only *after* the batch's notifications are sent, so a stream that dies mid-batch replays that batch rather than dropping it. `set_consumer_cursor` never moves a position back. There is no in-memory event source anywhere in `subscribe.rs`.
- **One generation, stopping cleanly.** The instance binds one key and one generation at startup and never re-reads them; the watchdog polls `identity.json` every two seconds and cancels the service on any change. Confirmed with a live stream open by the new test in defect 3; exit 13, as §10 requires.
- **Interest before the first pre-flight request.** `plan::execute` is correct — the assignment id comes from the stored plan, and the registration precedes `fetch_assignment`, with only local state reads before it. `submit` is not, and cannot be without a design change; see below. The merge did not move it: it sits where M6-c put it.
- **Security (§15).** The subscription writes nothing but cursor rows, needs no token and no network, and holds the identity lock *shared*, so it does not block ordinary commands — only the exclusive lock `identity remove` needs, which is the §3.4 behaviour it claims. Event rows keep allowlisted before/after fields only; no bodies, no tokens, no signed URLs.
- **URI construction.** `invalidated` interpolates the event's scope into a course URI, but the result is only ever compared against the accepted filter, every entry of which was parsed by `target_of` first. A malformed scope can match nothing.

## Needs a decision

**`submit` registers foreground interest after its first pre-flight request.**
REPORT §3.6 says "Register foreground submission interest before its first
preflight request." In `canvas submit` it cannot: an interest is keyed by
assignment (`interest.assignment_id`, and the lock file
`locks/interest-assignment-<id>.lock`), and the assignment id is the *output*
of the resolution read. So `session.validate_network_token()` and
`resolve_target` — both of which make requests — run unprotected, and only the
eligibility read, the freeze, and the post are covered. The window is short and
the consequence is priority, not correctness, and `plan::execute` has no such
window. Closing it needs one of:

1. a coordinator that can hold an interest not yet bound to an assignment
   (a second lock name, and a rule for how it is upgraded or released), or
2. an explicit reading that the resolution read is not a "pre-flight request"
   for the purpose of §3.6, in which case the sentence should say so.

This is a design question, not a bug to patch, so nothing was changed beyond
making the comment describe the code truthfully (defect 5). It is the same
placement M6-c shipped and `docs/reviews/code-M6-c.md` accepted; it is listed
here because the brief asked the question directly.

## Observations, not defects

- `canvas schema watch` and `canvas schema event` render the uniform `schema@1`
  page, which always carries an `envelope` and an `error` section. For
  `event@1` that is a little generous — a `--jsonl` line is self-describing and
  never wrapped — and for `watch@1` the page does not mention that `--json` is
  refused and `--jsonl` is the machine-readable form. The `EventJson` doc
  comment says both. The `schema@1` document shape is w2's contract and a
  shared file this lane does not own this round, so it was left alone.
- The subscription's dedup window is one batch of 500 rows, so a host that
  connects with no stored position replays the whole 30-day log and may see a
  URI named once per batch. It is at-least-once by design and the volume is
  bounded by four URIs per batch; worth knowing, not worth changing.
- `rmcp` answers the first request of a connection inline, so a connection
  whose *first* request is `subscriptions/listen` gets no other answer while
  the stream is open. The module documents it and the tests discover first, as
  a host does. This is an SDK property, not something this package can fix.
