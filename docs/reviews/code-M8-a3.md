# Code review — M8-a3 (lane `w3`)

**MERGE.** Item 4 is right where it counts: the count is a `Shape`, not a
second producer, so both callers reach one observation path and inherit M6-c's
baseline rules without a line of new event machinery; the payload is the count
and nothing else; and the two schema pages now describe the `--jsonl` stream
instead of an envelope that never wraps it.
Five defects were found and fixed on this branch: two missing tests the review
brief's attack list requires — an unknown count that could silently become a
zero, and a "no double-emit" claim nothing measured — plus three comments that
describe code they do not describe.
Every gate passes, `INSTA_FORCE_UPDATE=1` moves no snapshot byte, and
`cargo xtask bench --runs 3 --mcp --watch` meets every §13 target.

Scope: the follow-on only — `f438755`, `dae6758`, `8c6133c`, and the two
`main` merges (`d7b3fa5`, and this review's `f788bf3`, which brought
`tasks/review-code-m8a3.md` and nothing else). M8-a itself was reviewed in
`docs/reviews/code-M8-a.md` and `code-M8-a2.md`, M6-c in `code-M6-c.md` and
`code-M6-c2.md`.

## Gates

`CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/rev-m8a3`
(`.target/rev-m8a3-188` for the MSRV check, which needs its own).

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo nextest run --all-features` | **731 tests run, 731 passed, 0 skipped** |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | ok (MSRV holds; no `Cargo.toml` or `Cargo.lock` change, so Appendix A is untouched) |
| `INSTA_FORCE_UPDATE=1 cargo nextest run --all-features` | no snapshot drift: **no file changed** |
| `cargo xtask bench --runs 3 --mcp --watch` | every §13 target ok |

The bench page in one run, as `docs/bench.md` records it:

```
metric                       load          p50 ms    p95 ms   target p95 verdict
cached todo, first output    idle             7.2       7.5          150      ok
cached todo, full run        idle             7.7       8.1          250      ok
cold start                   idle             9.7      10.3          400      ok
cached todo, first output    download         6.7       6.8          150      ok
cached todo, full run        download         7.3       7.4          250      ok
cold start                   download        10.9      11.6          400      ok
cached todo, first output    watch            6.5       7.1          150      ok
cached todo, full run        watch            7.1       7.6          250      ok
cold start                   watch           11.3      11.5          400      ok
watch tick                   idle            21.7      21.8         none       —
warm todo.list over stdio    mcp              2.6       2.8          100      ok

catalog: 30 tools, 220855 bytes, ~55224 tokens per `tools/list`
```

The extra request the tick now makes for `inbox_unread` is in the tick figure:
21.7 / 21.8 ms against M6-c2's 26.4 ms p50 on the same machine. `docs/bench.md`
was left as the worker recorded it; this review's rerun was for the verdict,
not to re-record the page.

## The attack list, checked against the code

Each line was read in the source, not taken from the report.

| Question | Answer |
|---|---|
| First observation silent? | Yes. `observe::apply` writes the baseline and returns `events: 0` when `baselines` has no row. `the_unread_count_reports_only_what_changed` and the first `watch` tick both assert it. |
| Unchanged count emits nothing? | Yes. `compare::diff` compares `unread_count` only; equal values reach no bucket. Asserted at the unit and the end-to-end level. |
| Produced from the durable outbox? | Yes, and by construction: `refresh_dataset` → `ingest_success` → `events::observe_refresh` is the one hook, and `inbox_unread` gets it because `commit_refresh` writes its `membership` row generically. No producer of its own exists to drift. |
| A kill between the cache commit and the state transaction? | The observation id is `<dataset>:<scope>:<fetch_log rowid>:<fetched_at>`; a kill before `record` leaves the baseline untouched and the next refresh reports the same difference, a kill after it leaves a `pending` row `apply_pending` sweeps, and a cache row that moved underneath reports one `resync_required` and drops the baseline. `an_unread_count_observation_is_applied_once` covers the replay leg for this dataset; `a_kill_across_the_cache_and_state_handoff_replays_or_asks_for_a_resync` covers the general one. |
| Payload only the counts and the identity/scope fields? | Yes. `columns: &["unread_count"]`, `json_keys: &[]`, so `read_members` never selects `data_json` and the subject, the participants, and the message body of a conversation are not reachable from this shape. The unit test asserts the stored `before`/`after` bytes are exactly `{"unread_count":N}`. |
| Command and tick on the same path, no double-emit? | Yes. Both call `refresh_inbox_unread`; neither observes on a cache hit; the same `fetched_at` yields the same observation id, which `apply` refuses once it is `applied`. **Nothing measured this end to end** — defect 5. |
| `null` or unparseable count never a `0`? | Yes. `unread_to_entity` parses, keeps `None`, and `upsert_unread` writes SQL `NULL`; `observe::sql_value` maps `ValueRef::Null` to `Value::Null`. **Nothing measured it** — defect 4. |
| Schema pages match the output? | Yes, checked against the bytes: `canvas schema event` describes exactly the eleven fields the `m6c_watch_stream` snapshot line carries, with `schema` pinned; `canvas schema watch` still describes both envelope branches; and `Cli::validate` really does refuse `watch --json` as a clap `ArgumentConflict`, which is exit 2. |

Three further checks with nothing to report: no `Cargo.toml` or `Cargo.lock`
change, so Appendix A pins and MSRV 1.88 are untouched; §15 is unaffected — the
only new bytes on disk are one integer per identity in a table M8-a already
created, and the new wiremock route serves a fixture, not a recorded body; and
`Shape::compare_fields` → `Shape::changed` leaves `assignments`, `missing`, and
`announcements` byte-identical in behaviour, which the existing comparison
tests still hold.

## Defects found and fixed

| # | Severity | Where | What was wrong | What changed | Commit |
|---|---|---|---|---|---|
| 1 | Medium | `crates/canvas-core/src/events/tests.rs` (missing test) | `seed_unread` took an `Option<i64>` and every one of its four calls passed `Some`, so no gate held the package to M8-a's rule that a count Canvas does not report as a number stays unknown. `docs/reads-v2.md` states the rule and the payload shape as a contract. A change that turned `NULL` into `0` — in `sql_value`, in `upsert_unread`, or in a future `read_members` — would say the inbox is clear, which is the opposite of what is known, and every gate would still pass. | `an_unknown_unread_count_is_never_reported_as_zero` covers the three transitions the rule implies: known → unknown is one event whose `after` is `{"unread_count":null}`, unknown → unknown is silent, and unknown → known is one event whose `before` is `null` and not zero. | `1cb251b` |
| 2 | Medium | `crates/canvas-cli/tests/e2e/m6c.rs:347` | `the_unread_count_command_records_its_own_observation` asserted only that `notify` printed *a* line starting `inbox:`. One event and two events satisfy that equally, so the package's central claim — both producers reach one observation path, so a change is news once — had no gate. The claim is what makes item 4 a `Shape` rather than a second implementation, so it is the one thing worth measuring. | The test now runs a `watch` tick after the command over the same unchanged count, and checks the log still holds exactly one `inbox.unread_count`, both through the tick's own replay (with its `observed_at`, `before` and `after` pinned) and through `notify`'s per-kind count `(inbox.unread_count x1)`. | `7276f40` |
| 3 | Low | `crates/canvas-core/src/events/compare.rs:104`, `docs/reads-v2.md` | The shape's comment and reads-v2's first "choice where the report was silent" both justified `added: InboxUnreadCount` with "`added` can only follow a gap that dropped that baseline". It cannot: `report_gap` **deletes** the `baselines` row rather than emptying it, so the observation after a gap finds no baseline and is silent, exactly like the first one. With `refresh_inbox_unread` always writing its single row, no complete observation ever compares against a baseline that lacks it, so `added` is unreachable. The same paragraph said a row leaving the membership means the read failed; a failed read has `complete = 0` or `stale = 1` and is never observed at all. | Both now say `added` is named for completeness and never fires, and why the kind it names is still the right one; `removed: None` is justified by the row being either known or not known, with `resync_required` as the signal for an observation the log cannot trust. Behaviour unchanged. | `89c1f7b` |
| 4 | Low | `crates/canvas-core/src/events/compare.rs:40` | `Shape::changed`'s new doc said `due_at`, the score, and the grade "keep their own kinds wherever they are allowlisted". `missing` allowlists `due_at` for its payload and emits no `due.changed`, because `changed: None` turns field comparison off for the whole shape — the `compare_fields` flag it replaced was clearer about that. | The doc comment now says `None` turns field comparison off for the shape, and names `missing`'s `due_at` as the case that makes the difference. | `89c1f7b` |
| 5 | Low | `crates/canvas-cli/src/mcp/subscribe.rs:133` | `invalidated`'s comment asserted that beside `announcements` "no other dataset writes events". `inbox_unread` now does, so an invariant a reader would rely on when adding the next dataset was false. The mapping's answer is still right — no resource of a binding answers from `inbox_unread`, so claiming less is correct. | The comment names both datasets and says what a future one needs: a row here only when a resource reads it. | `81bce29` |
| 6 | Low | `crates/canvas-cli/tests/e2e/harness.rs:307` | `mount_inbox_unread` was inserted between `mount_submit`'s doc comment and `mount_submit`. The paragraph explaining why the submit mock echoes the body it was sent — without which `submission verify` reports `unavailable` for every attempt — became the unread-count mock's documentation, and `mount_submit` was left undocumented. | Moved the new function above the comment block. | `a0925b9` |

Defect 1 was confirmed to be a real gate and not a tautology by reading the
path it covers end to end: `UnreadCount.unread_count` is `Option<String>`,
`unread_to_entity` keeps `None` for anything that does not parse as an `i64`,
`upsert_unread` still writes the field so the column becomes `NULL` rather than
keeping a stale number, and only `sql_value` decides what `NULL` becomes in a
payload.

## Observations, not defects

- **The count prioritizes nothing.** REPORT §3.6 says activity-summary and
  unread counts "can prioritize checks". The tick refreshes `inbox_unread`
  last and uses the result for nothing but its own event. §3.6 says *can*, and
  `docs/reads-v2.md` records the placement and its reason as choice 3, so this
  is a permitted reading rather than a gap. Worth knowing if a later package
  wants the count to gate the coursework refreshes.
- **`output_schema` and the envelope-less document.** `mcp::catalog::output_schema`
  takes `document["envelope"]` and falls back to `{}`. A `schema@1` document
  can now legitimately lack that key, so a tool that ever named `event@1`
  would advertise an empty `outputSchema`. No tool does, and
  `every_output_schema_admits_both_shapes` fails loudly if one starts to, so
  the trap is armed rather than open.
- **Two identical `$comment` strings.** For the envelope form,
  `output.$comment` and `envelope.$comment` are byte-identical. The `output`
  block still earns its place through `form` and `flag`; the duplication is
  the cost of leaving the pre-existing `envelope` comment alone, which is the
  right call — changing it would churn every MCP tool `outputSchema`.
- **`docs/bench.md` is outside the M8-a brief's "docs/reads-v2.md only" rule.**
  The bench gate writes that file itself, so committing it is the honest
  record of the run rather than a docs edit.
- **The frozen clock is the only way to collide an observation id.** Two
  refreshes at one `fetched_at` produce one observation, and the second is a
  no-op. Under real time this cannot lose an event: the baseline moves only
  when an observation is applied, so a change the collided observation skipped
  is reported by the next one instead. The worker found this while writing the
  tests and gave each observing run its own instant, which is the right fix.

## Needs a decision

1. **`schema@1` is no longer one shape (SPEC §19, new item).** `canvas schema
   event` now drops `envelope`, `result` and `error` and carries `line`
   instead, while every other page keeps all three and gains `output`. The
   contract id stays `canvas-cli/schema@1`. §7's rule — "removing or changing
   a field bumps `n`" — governs §7 envelopes, and `canvas schema` is raw
   output with no envelope (§19 item 20 already records that `schema@1` has no
   registry row of its own), so the rule does not literally reach this
   document. The change is also the one `docs/reviews/code-M6-c2.md` asked
   for: the old `event@1` page described a wrapper that never exists. Decide
   whether the document is per-form by design, or whether a page that loses
   three top-level keys bumps the contract to `schema@2`. This review's
   reading, applied for the merge: per-form by design, because the page that
   changed was describing something untrue.

Nothing else was left unfixed.
