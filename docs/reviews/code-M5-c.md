# Code review — M5-c (end-to-end snapshot suite and exit-code precedence), branch `lane/w3`

Reviewer: Claude Opus 5 (high). Base: `e63e324` (merge of `main` into
`lane/w3`). Package brief: `tasks/m5c-e2e-suite.md`. Reviewer brief:
`tasks/review-code-m5c.md`. Spec: `docs/SPEC.md` §5, §7, §8, §13, §14,
§15, §16 row 3, Appendix A, Appendix D.

## Verdict

**MERGE-AFTER-DECISION.** The suite is the deliverable it was asked for:
every v1 command in both modes, all fourteen §14 exit codes, every
constructible precedence pair, the eleven §16 row 3 items mapped to named
tests by a test that checks the map, and a schema conformance pass over
every `--json` snapshot. Six `review(M5-c):` commits fix the two
cross-lane defects the worker reported plus four of my own, and all five
gates are green with no `#[ignore]`d test left in the workspace. One item
needs the owner: `auth status` emits a `pending_cleanup` field that
Appendix D's `auth_status@1` row does not list — a divergence that dates
from M0-c, not from this package, and that I cannot resolve without
editing the spec.

## Gate results

Run with `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/rev-m5c`
at `2e8b0c3`.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass |
| `cargo nextest run --all-features` | pass — 566 tests run, 566 passed, 0 skipped |
| `cargo deny check` | pass — advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | pass |

At the base the same run was **562 tests, 561 passed, 1 failed, 1
skipped**: `xtask::bench_smoke` failed on `main` itself, and
`precedence::auth_before_local` was ignored. Both are fixed below.

`cargo xtask bench --runs 3` was rerun after the fixture change; all six
metrics are inside their §13 targets and `docs/bench.md` carries the new
numbers.

The package adds no dependency and changes no version, so Appendix A is
untouched (`git diff main...HEAD -- Cargo.lock Cargo.toml` is empty). It
adds one Cargo feature, `canvas-core/test-support`, which only widens the
visibility of `test_scratch`.

## Defects found and fixed

| Sev | File:line | What was wrong | What I changed | Commit |
|---|---|---|---|---|
| High | `xtask/src/bench_fixture.rs:155` (before the fix) | The generated `bench-5` set answers no `/api/v1/announcements` and no `/api/v1/calendar_events`. M4-b made plain `sync` refresh announcements, so the priming `sync` in `xtask bench` saw a 404 per course, recorded `contexts_denied:404@course_101…105`, and exited 12; `Harness::prime` bailed and `bench_smoke` failed on `main` with no change of ours involved. `sync --full` would have failed the same way on the calendar. | Added both responses to the set, stored with no query pairs so `mount_set` matches whatever `context_codes[]` batch the client sends. The calendar covers the user context and all five courses, because a context with no answer is itself a denial; the user id became a named constant so the two agree. No all-day event: `bench` shifts a body by rewriting RFC 3339 timestamps and leaves a bare `all_day_date` alone, so an all-day row would drift out of the window. Regenerated the tracked set, extended the endpoint and decode tests, and added `the_calendar_covers_the_user_and_every_course`. | `c3b47a5` |
| High | `crates/canvas-cli/src/session.rs:141` (before the fix) | `Session::open` opened the identity store before it looked for a credential, so an invocation with no token *and* a store this binary cannot read reported exit 13. §14 orders the aborts 2, 3, 13, …, so exit 3 must win. The worker left `precedence::auth_before_local` `#[ignore]`d for this. | `CANVAS_TOKEN` is now read before `OpenIdentity::open`, and a store that refuses to open reports the missing token instead when there is no token and the command is not `--offline`. Nothing else moves: a resolvable token is not a precondition for a session — a class-B command with cache coverage still runs without one — and only a store that will not open makes the network unavoidable, so the two conditions overlap exactly where §14 puts 3 first. `--offline` and the class-A/B local commands keep exit 13, which `local_before_network` and `local_before_offline_miss` assert. Test un-ignored; the whole `canvas-lms-cli` suite, M0-c auth included, passes. | `66e87e5` |
| Medium | `crates/canvas-cli/tests/e2e/precedence.rs:14` (before the fix) | The brief names two constructions for the completed-command order; only `10 > 12` (`download --verify`) had a test. The module header recorded 9 as unpairable — "9, 8 and 11 are terminal … so no invocation ever carries one of them next to a lower-ranked outcome" — which is wrong for 9. | `recovery_outranks_a_partial_upload`: a `submit --file` whose upload session is refused leaves the journal `upload_incomplete`, which §14 ranks 9. The test asserts exit 9, `outcome: recovery`, the two frozen files still reported, and `partial: []` — a partial upload never becomes the exit 12 a dataset command would use. `CanvasServer::allow_file_submission` was added to reach the path, since the shipped assignment fixture has already spent one of its two attempts. Header and `docs/testing.md` corrected. | `8d50e99` |
| Medium | `crates/canvas-cli/tests/e2e/selection.rs:61` (before the fix) | §16 row 3's last item asks for "unit-level on the key function, **plus a `cfg(windows)` path test**". Only the portable rules check existed, and it runs on every platform *except* the one that enforces the rules. | Added `ipv6_origin_identity_key_is_a_real_windows_directory`, which creates the key as a directory and reads the name back, so on Windows the file system itself — device names, trimmed trailing dots and spaces — decides whether the slug is storable. Named in the row 3 table, which `every_row_3_item_names_a_test_that_exists` verifies. | `aebb8bd` |
| Low | `crates/canvas-cli/tests/e2e/commands.rs:52` (before the fix) | `doctor` was snapshotted only with `--network`. §5 gives it two forms, and the one a class-B invocation actually runs — network checks reported as `skipped` — had no test. | `doctor_without_network` snapshots both modes of the local form and asserts each of the three network checks is `skipped` and that `requests.api` is 0. | `14afe1f` |
| Low | `crates/canvas-cli/tests/e2e/schema.rs:275` (before the fix) | A dead `if let … && … { let _ = base; }` block sat in `check_ids_and_local_siblings`, reading as the `ts+local` sibling check the module doc promises while doing nothing. | Removed it and corrected the doc: the sibling rule is enforced by `compare`, which requires the live key set to equal the fixture's in both directions. | `14afe1f` |
| Low | `crates/canvas-cli/src/session.rs:293` (before the fix) | The rewrite that pointed `config_dir` at `CliPaths` left the old summary line above the new one, so the item carried two contradictory first lines. | Dropped the stale line. | `2e8b0c3` |

## What I checked and found correct

- **§16 row 3, item by item.** All eleven named items are mapped in
  `docs/testing.md`, and `selection::every_row_3_item_names_a_test_that_exists`
  parses that table, resolves each `file.rs::function`, and asserts the
  count is eleven — so the map cannot rot into names that no longer
  exist. Nine items are covered by the package that introduced the
  behaviour, which is what the brief asked for ("do not duplicate a case
  an earlier package already covers"); the two with no end-to-end test
  are asserted in `e2e/selection.rs`.
- **§5 command coverage.** Every command in the §5 block has a table and
  a `--json` snapshot, including `sync --full`, `receipts acknowledge`,
  `submission reconcile` and the three `open` subcommands. The four
  raw-output commands (`completions`, `receipts export --out -`,
  `calendar --ics -`, `auth token --reveal`, plus `config edit`) are in
  `raw_output.rs` with the `--json` usage error on each, asserted to be
  exit 2 with an empty stdout.
- **§14 exit codes.** One test per code, 0 through 13, each asserting the
  schema and the `outcome` as well as the code — so an abort (the `error`
  schema) stays distinguishable from a completed command with a
  non-success outcome. Exit 13 gets both of its §14 causes: a newer cache
  schema and a credential store that refuses.
- **§14 precedence.** Every adjacent abort pair a single invocation can
  hold is constructed: 2>3, 2>6, 3>13, 3>4, 13>4, 13>7, 5>6, 4>6, 6>7,
  with a companion assertion for 6>7 that the same invocation with a
  resolvable operand really does report 7. The two pairs that are shown
  as unconstructible — 4 before 5, and 8/11 beside anything lower — are
  argued in the module header rather than skipped silently, and I agree
  with both arguments after checking that `submit` never emits
  `Outcome::Partial` (its outcome comes only from the journal state).
- **§7 envelope and schema conformance.** `schema.rs` reads every JSON
  snapshot, checks the fixed eleven-key envelope, that `freshness`,
  `partial` and `warnings` are never null, that ids are strings, and that
  no `_local` field stands without its base; then it compares the
  `result` against the registry fixture for its schema with key sets
  equal in both directions, nullability declared by the fixture (with an
  explicit `NULLABLE_WITH_EXAMPLE` allowlist I spot-checked against the
  Appendix D `T?` markers), arrays never null, and the Appendix D `Sort`
  column per schema. `the_shape_table_covers_every_registry_fixture`
  makes the fixture directory and the shape table agree, so a new
  `result` shape cannot land unchecked.
- **Appendix D multi-shape rows.** The registry gained a `variant` field
  so `receipts@1` (list/show/export/acknowledge), `cache@1`
  (stats/clear/path), `config@1` (get/set/path) and `identity@1`
  (list/remove) each carry one fixture per shape, and the duplicate
  check is now keyed on `(id, variant)`. The four rows that list several
  shapes in Appendix D are exactly the four that got variants. The old
  single `cache.json` was replaced by `cache_stats.json`, which is the
  same shape under an honest name.
- **§15 security.** No snapshot contains the fixture token — the harness
  scrubs it to `<token>`, and `auth token --reveal` is asserted to print
  the secret and nothing else while its snapshot shows only the
  placeholder. No fixture-server port or host leaks into a snapshot. The
  exported receipt is asserted to be the document itself, not an
  envelope, and nothing writes a raw response body.
- **Test-only environment gates.** `CANVAS_NOW`, `CANVAS_TEST_FORCE_FILE`,
  `CANVAS_TEST_KEYRING_ERROR`, `CANVAS_TEST_CRASH_AFTER`,
  `CANVAS_TEST_ALLOW_HTTP` and the new `CANVAS_TEST_NO_LAUNCH` all carry a
  `cfg!(debug_assertions)` gate; I checked each call site. The package
  closed a real hole here: `CANVAS_NOW` was previously honoured in a
  release build, where it would have moved due dates, TTL freshness and
  the §12.2 thirty-minute `--assume-not-submitted` window. `doctor`'s
  clock-skew check deliberately keeps the wall clock, with a comment
  saying why.
- **Determinism.** `COLUMNS=100`, `--color never`, `TZ=America/New_York`
  and a frozen `CANVAS_NOW` are set per test; each test gets its own
  config root, data root and scratch from `test_scratch::Scratch`, so the
  suite runs in parallel and cleans up. The genuinely volatile values —
  journal and receipt UUIDs, journal `created_at`/`updated_at`, the
  identity DB size, the server port, the crate version, the build target
  and commit, `doctor`'s skew, and the `receipts export` byte count — are
  each masked with a comment saying why, and `receipts export` asserts
  the reported count against the file on disk instead of pinning it. I
  ran the suite twice with no `INSTA_*` variables; nothing moved.
- **MSRV and dependencies.** `cargo +1.88 check --workspace --all-targets`
  passes. The only manifest change is the new `test-support` feature on
  `canvas-core`, which gates `pub mod test_scratch` behind
  `#[cfg(any(test, feature = "test-support"))]`; no dependency, version or
  pin moved.

## Needs a decision

1. **`auth status` emits a field Appendix D does not list.** Appendix D's
   `auth_status@1` row is
   `{ profile?, identity?, token_source?, stray_sources: [string], backend?, validated_at? }`.
   The command has emitted a seventh field, `pending_cleanup: [string]`,
   since M0-c (`commands/auth.rs:267`); it reports the `cleanup_keyring`
   and `cleanup_file` flags §16 row 3 requires a failed logout to leave
   behind, so the behaviour is right and the spec row is what is stale.
   The registry fixture was empty of it until this package, which is why
   the new conformance test surfaced it.

   Two ways out: add `pending_cleanup: [string]` to the Appendix D row, or
   move the flags into `stray_sources` and drop the field. I recommend the
   first — the flags mean something different from a stray entry (one is
   "a deletion we owe", the other "a credential we did not put there"),
   and `doctor` already reports them separately. Either way it is a spec
   or a schema change, not something a reviewer should guess, so the
   fixture keeps matching the code and the code is unchanged.

## Notes, not defects

- `every_json_snapshot_matches_its_registry_fixture` reads the tracked
  `.snap` files rather than output captured in the same process, so on its
  own it validates the committed snapshots rather than live behaviour.
  That is sound only because every other test in the suite asserts the
  same snapshots against live output in the same run; worth keeping in
  mind if the suite is ever split.
- The URL operand forms (`assignment <url>`, `announcement <url>`,
  `open <url>`, `submit <url>`) have no end-to-end snapshot. They are
  alternate operand spellings rather than separate commands, and §6
  resolution for each is covered by the package that introduced it
  (`review_m2b_integration`, `open.rs`'s origin unit tests), so I left
  them out under the brief's no-duplication rule.
- `check_ids_and_local_siblings` accepts an `*_id` string that is neither
  all digits nor a UUID as long as it contains a non-alphanumeric
  character. It is loose, but every id the suite actually produces is one
  of the two, and tightening it would need a per-field rule.
- The bench fixture deliberately carries no all-day calendar event; the
  reason is in a comment on `calendar_event`. If `xtask bench` ever grows
  a date-shifting rule for bare `YYYY-MM-DD` values, that restriction can
  be lifted.
