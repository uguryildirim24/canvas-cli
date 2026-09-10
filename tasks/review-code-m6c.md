# Code review + fix — M6-c on branch lane/w3 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M6-c
on branch `lane/w3` (worktree `/Users/rolfie/projects/canvas-cli/.worktrees/w3`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /Users/rolfie/projects/canvas-cli/.worktrees/w3`. Read the package brief `tasks/m6c-coordinator-watch.md` and the SPEC sections it
   cites. First run `git merge main` (expect nothing to do; resolve if needed and commit). Then read `git log main..HEAD --stat` and the full diff. Scope: the package is delivered without its item 7 (the MCP `subscriptions/listen` hookup), which waits for M6-b on `main` and will be a separate follow-on; do not count item 7 as missing. Its contract is `docs/agent-ux/REPORT.md` §3.6 (all of it) and §3.4 (identity leases), with `docs/SPEC.md` §10 and §11 unchanged for what a dataset, a TTL, and a governor sample mean. Attack in particular: can two processes ever exceed `api_concurrency` in flight (count on the wiremock side, not in process)? Does the shared governor apply the §11 rules to the shared values (lower sample always, higher only above the watermark, cost pre-charged, refill never above 10/s, cooldown shared) and never invent a full bucket after an owner dies? Is any network wait inside a database transaction? Is the refresh single-flight lock keyed so two distinct scopes cannot share a file, and does the 30 s waiter serve honest stale/partial metadata? Can `watch` starve a foreground submit at concurrency one, or can a pending journal freeze readback of an `outcome_unknown` journal? Events: first baseline silent, partial page never emits a removal, `grade.posted` only with `posted_at`, kill between the cache commit and the state transaction yields replay or `resync_required` and never a silent gap, expired or foreign-generation cursor emits `resync_required` and exits 0, 30-day retention expires rows transactionally and never the file, `cache clear` leaves the state tables alone, no payload carries a token, signed URL, full message body, or DOM text. Check that every existing governor test still passes unchanged and that the §13 `todo` targets hold with a `watch` process running (rerun `cargo xtask bench --runs 3` and `--watch --runs 3`). The worker's final report, for reference:

```
Delivered
  1. Cross-process permits and shared governor: Permits/GovernorState seams in
     canvas-api (no behaviour change in process), canvas-core::coord with
     api-slot-<n>.lock and the governor row.
  2. Refresh single-flight through canvas-core::sync, wired into every refresh_*.
  3. Foreground interest, registered by the submit command and by plan::execute
     before their first pre-flight request. M6-a is on main, so this call site is
     wired, not deferred.
  4. Events: migration 0003_events, the observation outbox, baselines, the 11
     kinds, 30-day retention.
  5. canvas watch --jsonl [--since CURSOR] [--once], with event@1 and watch@1 in
     the registry and fixtures.
  6. canvas notify [--since CURSOR] [--stdout].
  7. The test list, plus cargo xtask bench --watch and the docs/bench.md section.
  Item 7 is not done. canvas mcp is not on main (git log main carries M6-a but no
  M6-b merge), so subscriptions/listen has nothing to hook into.
  Dependencies for Appendix A: only tracing was added, to canvas-core (already a
  workspace dependency used by two other crates), plus the time tokio feature
  there. notify-rust was rejected: its macOS backend mac-notification-sys carries
  Objective-C FFI, i.e. unsafe code. --stdout is therefore the only notify
  backend; without --stdout the command warns on stderr that no desktop backend
  exists and writes the same lines to stdout, so it never claims a notification
  it did not post.
  Undefined cases, and the reading I chose (each one emits less, or claims less)
  - watch without --jsonl prints a short human line per event. --json is refused
    with exit 2 in both forms.
  - The stream always ends with one watch@1 document, not only under --once: a
    consumer that reads to EOF always learns the final cursor. This is the only
    extra document watch writes.
  - event@1 is a flat, self-describing line, not a §7 envelope: schema, cursor (a
    string id), kind, observed_at + observed_at_local, identity, generation,
    dataset, scope, entity_key, before, after. §7's one-document rule is
    untouched.
  - --since 0 claims no position, so only the generation is checked; a foreign
    generation still forces resync_required.
  - A tick skips polling for foreground interest first, then for a journal in
    state planned|uploading|uploaded|posting. A terminal outcome_unknown journal
    is never counted, so it cannot freeze polling.
  - A failed scope backs off (30 s, doubling to 15 min) and every other dataset
    keeps its own schedule; only an authentication failure ends the run. A tick
    that skips reports skipped and refreshes nothing rather than reporting stale
    coverage as new.
  - notify is a raw-output command (--json → exit 2). It posts one line per
    event-kind group and keeps its cursor in a new consumer_cursor table in
    migration 0003_events; the cursor moves only after the lines are written, so
    a failed run repeats rather than drops.
  - watch refuses --offline with exit 2, like sync. notify runs offline: it reads
    only the local log.
  Two behaviour fixes found by the new tests
  - The single-flight lock now covers the cache commit as well as the fetch. It
    was released when the last page arrived, so a waiter re-read an empty cache
    and fetched the same scope again.
  - The waiter's freshness window is the caller's clock, not Timestamp::now(), so
    a frozen test clock and a real one compare the same way.
```