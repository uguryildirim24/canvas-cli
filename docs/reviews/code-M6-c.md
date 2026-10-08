# Code review :  M6-c (shared coordinator, `watch --jsonl`, events, `notify`)

Branch `lane/w3`, worktree `<checkout>`.
Reviewed against `docs/agent-ux/REPORT.md` §3.6 and §3.4, `docs/SPEC.md`
§7, §9-§12.2, §13-§16 and Appendix A/D, and the package brief
`tasks/m6c-coordinator-watch.md`. Item 7 (the MCP `subscriptions/listen`
hookup) is out of scope by instruction and is not counted as missing.

## Verdict

**MERGE.** Five defects were found and fixed on the branch; four were in
the cross-process governor and the priority rule, where the package's own
tests could not see them because they never exercised two live values at
once. Everything the brief lists as an acceptance test exists, runs a real
second process where it must, and passes; all seven gates are green and
the §13 targets still hold with a `watch` process running.

## Gates

Run with `CARGO_TARGET_DIR=<checkout>`,
on the review head `2260c9f`.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass, no warnings |
| `cargo nextest run --all-features` | pass, 627 tests, 0 skipped (622 before the review's 5 new tests) |
| `cargo deny check` | pass :  advisories, bans, licenses, sources all ok |
| `cargo +1.88 check --workspace --all-targets` | pass |
| `cargo xtask bench --runs 3` | pass, every §13 target ok |
| `cargo xtask bench --watch --runs 3` | pass, every §13 target ok under the `watch` load; watch tick p50 21.7 ms / p95 21.9 ms |

Under the `watch` load: cached `todo` first output p50 6.6 ms / p95 8.3 ms
(target p95 150), full run p95 8.9 ms (target 250), cold start p95 11.1 ms
(target 400). Recorded in `docs/bench.md`.

No new dependency: `tracing`, `fs4` and `sha2` are already in Appendix A
and the `tokio` `time` feature is already listed there. `notify-rust` was
rejected for Objective-C FFI on macOS, so `--stdout` is the only `notify`
backend and the command says so on stderr. Appendix A needs no change.

## Defects found and fixed

Line numbers are in the pre-review tree (`a427071`). All five fixes are on
`lane/w3`; nothing was pushed or merged.

| # | Sev | Where | What was wrong | What I changed | Commit |
|---|---|---|---|---|---|
| 1 | High | `crates/canvas-api/src/governor.rs:515` (`publish`), `:566` (`merge_shared_locked`) | With a shared `governor` row the §11 estimate could only fall. `publish` re-merged the stored row after `observe` applied a sample, and the conservative rule "a lower estimate always applies" then adopted the process's **own pre-charge**, which is older evidence than the header just applied. Ten responses each reporting a full 700 left the shared estimate at 690; a long-running process walked it down one cost per request into a permanent cooldown. In process §11 already lets a header replace the estimate outright. | Split the merge into `Merge::Admission` (unchanged conservative rule) and `Merge::AppliedSample`, where only a strictly newer watermark may displace the sample just applied. New test `a_sample_survives_the_shared_row_instead_of_ratcheting_down`. | `2044198` |
| 2 | High | `crates/canvas-api/src/governor.rs:515` (`publish`) | The same re-merge read back the `cooldown_until` the process had published on its own cooldown admission and re-armed the flag, so §11's "cooldown ends when an applied sample is ≥ 300" never took effect with a shared row: every later request waited out the five-second probe forever. | A live cooldown is still shared at admission, but is not restored over the sample that ended it unless the row is strictly newer. New test `a_shared_cooldown_ends_on_an_applied_sample`. | `2044198` |
| 3 | Medium | `crates/canvas-api/src/governor.rs:362` (issue counter) | §11 orders samples by issue number, and the counter is per process. A watermark another process wrote sat above every issue this one could hand out, so none of its own higher samples could apply and its estimate could only fall :  a fresh CLI next to a long-running `watch` stayed in the other process's cooldown until that process happened to publish a recovery. | `align_issue_locked` continues this process's sequence above any adopted watermark, after every shared-row merge. New test `a_process_that_adopts_a_watermark_can_still_apply_its_own_sample`. | `2044198` |
| 4 | Medium | `crates/canvas-cli/src/commands/watch.rs:288` (`tick`) | Foreground priority was read **once per tick**. A `submit` that registered interest after the tick began waited behind every remaining refresh of that tick. The new test measures it at `api_concurrency = 1`: five further admissions (enrollment_grades, assignments, missing, planner, announcements) went out while the submission waited, instead of the one request already in flight that REPORT §3.6 allows. | `attempt` re-reads the priority state (foreground interest and the §10 pending hook) before it polls each refresh future; the futures are lazy, so nothing is admitted before the check. New e2e test `interest_that_arrives_during_a_tick_stops_the_rest_of_it`, which registers interest from a second process while the tick's first request is in flight. | `d57c953` |
| 5 | Medium | `crates/canvas-api/src/governor.rs:592` (`reset_silence_locked`) | With a shared row the §11 header-silence window belongs to the row, not to one process, but the reset still measured local clocks. A `watch` that had polled nothing for a minute reset its estimate to full and cleared `in_cooldown` on its next admission :  immediately after `adopt_shared` had read a cooldown a live process published a moment earlier. `charge` re-merged the row afterwards, so the estimate recovered, but the admission decision had already been made against the invented full bucket and the request went out with no probe wait at all. | The silence reset defers to `merge_locked`, which already implements the §11 reset for the shared row, whenever a row has been read inside the window. Without a shared row nothing changes. New test `a_quiet_process_cannot_reset_a_shared_row_another_one_keeps_fresh`. | `db301af` |
| 6 | Low | `crates/canvas-core/src/coord/interest.rs:70`, `:130`; `crates/canvas-core/src/events/log.rs:244` | Three new durable writes used a bare `execute`, which SQLite runs as a deferred transaction that upgrades late and can return `SQLITE_BUSY` instead of waiting out `busy_timeout`. SPEC §10 requires `BEGIN IMMEDIATE` for every write transaction. Two of the three swallow their error, so a `notify` run that lost the race would have repeated its alerts. | All three now open `BEGIN IMMEDIATE`; `set_consumer_cursor` takes `&mut Connection`. | `e47ce4d` |

`2260c9f` re-records `docs/bench.md` on the review head.

Every pre-existing governor test passes unchanged: the 22 tests in
`crates/canvas-api/tests/governor_tests.rs` were not edited, only added to
(25 now, plus the two seam tests the package already had are untouched).
The in-process path is bit-for-bit the old behaviour :  every change above
is inside a `self.shared.is_some()` branch or gated on a shared row being
read.

## What I attacked and found sound

- **Two processes cannot exceed `api_concurrency`.** Counted on the
  wiremock side by `Slow::peak()`, not in process:
  `one_api_slot_bounds_a_cli_command_and_a_watch_together` runs a real
  `watch` and a real `todo --fresh` against one server at
  `api_concurrency = 1` and asserts the server never saw two requests
  overlap. `flock` is per open-file-description, so the cap also holds
  between tasks inside one process. A dead owner frees its slot with its
  descriptor (`a_dead_owner_releases_its_api_slot`, real subprocess).
- **No network wait inside a database transaction.** `GovernorState::update`
  is handed a pure merge closure; the fetch, the pagination and the
  single-flight wait all happen outside `store.call`. Confirmed by reading
  every call site in `coord/`, `sync/refresh.rs` and `events/observe.rs`.
- **A vanished owner never gets a full bucket handed to it.** The stale-row
  path is exactly §11's reset, and only with nothing in flight
  (`a_vanished_owner_leaves_a_row_that_the_silence_rule_resets`,
  `a_stale_shared_row_resets_exactly_as_the_silence_rule_says`). Defect 5
  was the one way an invented full bucket could still appear.
- **Single-flight keying.** `refresh_lock_name` escapes every byte outside
  `[a-z0-9._]` as `~hh`, including `~` and every upper-case letter, so the
  encoding is injective, contains no `-` of its own (the two separators
  stay unambiguous) and cannot be folded by a case-insensitive filesystem.
  The >200-character digest fallback contains no `-`, so it can never
  collide with the escaped form. Both proven by test.
- **The 30 s waiter is honest.** A waiter that times out serves the cache
  row with `source: cache`, `stale: true` and the row's own `complete`
  and `error`; with nothing usable it reports the §14 exit 13 lock
  timeout, which is the listed meaning of 13. A `--fresh` waiter accepts
  the holder's row only when `fetched_at >= now` for the caller's own
  clock. `a_waiter_that_times_out_serves_what_the_cache_has` asserts the
  waiter made no second fetch.
- **An `outcome_unknown` journal cannot freeze polling.** Only
  `planned|uploading|uploaded|posting` count as in flight
  (`an_unknown_outcome_never_stops_polling_but_an_active_one_does`).
- **Events.** First complete observation is silent; only a `complete = 1,
  stale = 0` `fetch_log` row is observed at all, so a partial or failed
  page can never imply a removal; `grade.posted` needs `posted_at` to go
  from null to non-null and is emitted instead of, not as well as,
  `grade.changed`; a kill between the cache commit and the state
  transaction replays or emits exactly one `resync_required` and never a
  silent gap (real killed subprocess, all three phases); an expired or
  foreign-generation cursor emits `resync_required` and exits 0; retention
  deletes rows in one `BEGIN IMMEDIATE` and `AUTOINCREMENT` keeps
  `sqlite_sequence` so a cursor is never reused.
- **`cache clear` cannot touch any of this.** `cache_clear` takes only the
  cache connection, so it is structurally incapable of reaching
  `state.sqlite`; `cache_clear_cannot_reset_epoch` already covers the
  durable side.
- **§15.** Event payloads are the allowlisted cache columns only (name,
  `due_at`, `points_possible`, submitted, graded, score, missing,
  `workflow_state`, attempt, grade, `posted_at`, announcement title). No
  token, signed URL, message body or DOM text can reach a row: blobs are
  written as null and nothing reads a response body. `notify` and `watch`
  add no new persisted field.
- **§7.** `watch` refuses `--json` with exit 2 and `notify` is registered
  as a raw-output command, so the single-document rule is untouched;
  `event@1` and `watch@1` are registered with fixtures and snapshots.

## Needs a decision

1. **The coordinator opens a second `state.sqlite` connection, outside the
   store's single SQLite thread.** SPEC §10 says "All SQLite access runs on
   one dedicated thread per process fed by a bounded channel"; the
   coordinator uses its own `Arc<Mutex<Connection>>` from the async
   runtime thread instead (`coord/mod.rs`, `Coordinator::open`). The
   package documents why :  the governor row is written on the request path
   and must not queue behind the command's own database work :  and WAL
   plus `busy_timeout` make it safe for correctness. The cost is that the
   CLI's `current_thread` runtime can be blocked for up to the five-second
   `busy_timeout` on a contended `BEGIN IMMEDIATE`, stalling the timers of
   requests already in flight. Fixing it means either an async
   `GovernorState` seam or a second dedicated database thread; both are
   larger than a review edit and one of them changes §10's stated rule.
   Left as the worker wrote it.
2. **Foreground priority is now bounded per refresh, not per request.**
   After fix 4, a refresh already admitted may still issue the remaining
   pages of its own dataset while a submission waits. Bounding it to the
   single request §3.6 names would mean gating inside permit acquisition,
   which would park a `watch` tick *while it holds that scope's
   single-flight lock* :  and the foreground's own refresh of the same
   scope would then wait out its 30 s waiter, which is worse than what it
   fixes. At `api_concurrency = 1` the residual wait is one dataset's
   pagination (one request for every dataset in the bench fixture).
   Whether that residue is acceptable, or whether the permit layer should
   learn about priority, is a design call.
3. **`canvas watch` with no `--since` replays the whole retained log** (up
   to 30 days) before streaming. REPORT §3.6 defines replay only relative
   to a cursor, and the package rule was to prefer the reading that emits
   fewer events; the alternative is to start at the head and let a
   consumer ask for history with `--since 0`. The worker named the
   `--since 0` reading but not this one. Cheap to change either way.
4. **A cold cache plus a running `watch` can turn a foreground `--fresh`
   read into exit 13.** If `watch` holds a scope's single-flight lock for
   longer than the 30 s waiter and the cache has no complete row for that
   scope, the foreground command reports the lock timeout rather than
   fetching. This is REPORT §3.6's stated fallback ("the normal miss/error
   when nothing usable exists") and only `submit`/`plan execute` register
   the interest that would stop `watch` polling, so `todo --fresh` on a
   first run is the exposed case. Confirming that this is the intended
   trade-off is Rolf's call.
