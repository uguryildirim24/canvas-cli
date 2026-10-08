# Code review :  M6-a (operation plans and approval core), branch `lane/w1`

Reviewer: Claude Opus 5 (high). Base: `f2137a6` (merge of `main` into
`lane/w1`). Package brief: `tasks/m6a-plans-approval.md`. Reviewer brief:
`tasks/review-code-m6a.md`. Contract: `docs/agent-ux/REPORT.md` §3.2 and
§3.5 for the plan layer, `docs/SPEC.md` §12.2 from the journal insert
onward, plus §10, §14, §15, §16 row 2 and Appendix D.

## Verdict

**MERGE.** The approval guarantee holds: after the refactor no CLI path
reaches a journal except through `plan::execute`, execute refuses an
expired, invalidated or unapproved plan before it opens the network, and
the one state transaction that consumes the approval, inserts the journal
and marks the plan `executed` is guarded twice :  by the unique index on
`submission_journal.plan_id` and by `UPDATE plans … WHERE state =
'approved'` :  so one plan admits exactly one journal under two racing
processes, a kill at either side of the commit, and a replay. Four
`review(M6-a):` commits fix one real behaviour defect and three test and
contract gaps I found; all five gates are green. Four items are listed
under "Needs a decision"; none of them blocks the merge, and the largest
is that `docs/SPEC.md` Appendix D does not yet carry the two fields the
code now emits, which the worker was forbidden to add.

## Gate results

Run with `CARGO_TARGET_DIR=<checkout>`
at `81994a2`.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass |
| `cargo nextest run --all-features` | pass :  588 tests run, 588 passed, 0 skipped |
| `cargo deny check` | pass :  advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | pass |

At the base (`f2137a6`) the same five gates were already green with 583
tests. The package adds no dependency and changes no version:
`git diff main..HEAD -- Cargo.toml Cargo.lock crates/*/Cargo.toml` is
empty, so Appendix A is untouched. `plan` uses `uuid`, `sha2`, `jiff`,
`rusqlite` and `serde_json`, all of which `canvas-core` already carried.

One caveat on the runner: a single `cargo test -p canvas-lms-cli --test
e2e` run failed `commands::cache_stats_path_clear` and then passed four
consecutive times, on both the base and the final tree. `cargo test` runs
the whole e2e binary in one process; `cargo nextest`, which §16 names as
the runner and which the gate uses, isolates each test and is green. I did
not chase it further and it is not caused by this package.

## Defects found and fixed

| Sev | File:line | What was wrong | What I changed | Commit |
|---|---|---|---|---|
| Medium | `crates/canvas-core/src/plan/execute.rs:110` (before the fix) | Execute reads the plan twice: once before the revalidation `GET`, once under the admission lock. The second read asked only whether the plan was already `executed`. A `decline`, a `cancel` or an expiry that landed while the `GET` was in flight therefore travelled all the way to `create_linked`, where the `state = 'approved'` guard matched zero rows. **No journal was ever published** :  the guard is the real defence and it held :  but the refusal came back as `approval_required` with "the approval was consumed by another execute", naming a race that did not happen instead of REPORT §3.2's real reason, and a journal id and its owner-lock file were minted for a row that then rolled back. | The read under admission now re-runs `guard_admission` and the approval check, so a plan declined during the read is refused as `invalidated` carrying the decline's own text, before anything is minted. Added `a_decline_during_the_revalidation_read_is_named_as_the_decline`, which declines during a delayed revalidation `GET` and asserts the reason, that no journal exists, and that the mock saw no write; it reports `approval_required` without the fix. | `72f4a98` |
| Medium | `crates/canvas-cli/tests/e2e/schema.rs:92` (before the fix) | `NULLABLE_WITH_EXAMPLE` declares which Appendix D `T?` fields may be null under a fixture that shows an example. `receipt@1`'s new `plan_id` and `approval`, the same two on the `receipts@1` journal rows, and `plan@1`'s `approval` all show examples in their fixtures and none was listed, so the conformance checker read every one of them as always-present. A journal created before plans :  whose shape the brief and REPORT §3.5 define as exactly `null` for each :  would have been reported as a shape violation rather than as the correct legacy record. `plan@1`'s `approval` is null for every plan that has not been approved yet, which is most of them. | Added the nine paths, plus the four `plan@1` assignment fields its fixture fills in, and a test that nulls the plan fields in every registry fixture and re-checks it, so a shape that carries them cannot quietly declare them mandatory. Nothing in the suite would have caught this today: every submission in the e2e suite runs through the plan path, so no snapshot carries the legacy shape yet. | `81994a2` |
| Low | `crates/canvas-core/src/store/migrate.rs:24` (before the fix) | The migration list became a real list :  `migrate_state` and `migrate_cache` now take the version the database has reached and branch on it, and `open_db` passes it :  but no test ever opened a database that had already run `0001`. Every test opens a fresh file, so the entire upgrade path, on both databases, was unexercised, as was the claim that a journal written before plans exposes `plan_id` and `approval` as null. | Three tests over the batch list: a v1 state database gains `plans`, `approval_handles` and the unique journal link while its existing journal row reads null for both new columns; the unique index admits one journal per plan and keeps null plan ids distinct, so legacy rows never collide; and a database already at the current version runs no batch, which re-running `0001` would fail. | `1242c4b` |
| Low | `crates/canvas-cli/tests/e2e/harness.rs:883` (before the fix) | The raw-stdout scrubber writes one hard-coded placeholder, so adding `plan_id` and `plan_sha256` to its key list stamped both with `<clock>`: the `receipts export --out -` snapshot recorded a plan id as a clock reading and a 64-character digest as another one, contradicting the `VOLATILE_KEYS` table two hundred lines below that gives them `<id>` and `<digest>`. | The placeholder is a parameter now and each key keeps the name `VOLATILE_KEYS` already gives it; snapshot updated. | `06ed247` |

## What I checked and found correct

- **Nothing dispatches a remote write without a recorded approval.** The
  only route to a journal in the CLI is `plan::execute` → `link` →
  `create_linked`, and `link` refuses a plan whose `approval` is `None`
  while `create_linked`'s `UPDATE plans … WHERE plan_id = ? AND state =
  'approved'` refuses one that is not approved. `state = 'approved'` is
  reachable only through `approve`, which spends a handle
  (`UPDATE approval_handles SET used_at = ? WHERE handle = ? AND used_at
  IS NULL`) inside the same immediate transaction; `issue_handle` only
  issues against a `prepared` plan; nothing moves a plan back to
  `prepared` or `approved`. `submit` records `tty` for an answered prompt
  and `yes-flag` for `--yes`, never the reverse, and cancels the plan on
  a declined or unanswerable prompt so it can never be executed later.
  `submit::create_from_plan` and `preflight_with_input` are still public
  on `canvas-core` and still create an unlinked journal, but no binary
  calls them any more :  only the §12.2 tests do. **M6-b must route
  `submission.execute` through `plan::execute`, not through those.**
- **One plan never admits two journals.** Two guards, both inside the
  journal insert transaction: the partial unique index
  `submission_journal_plan ON submission_journal(plan_id) WHERE plan_id IS
  NOT NULL`, and the plan's own expected-state guard. Nulls stay distinct
  in SQLite, so legacy rows do not collide :  asserted by the new
  migration test. `concurrent_executes_and_a_replay_create_exactly_one_journal`
  proves it with two real operating-system processes released together
  plus a third replay, and counts the journal table at the end.
- **The kill test brackets the commit, not a mock of it.** The helper is
  re-invoked as a child, `CANVAS_JOURNAL_PHASE` parks it at `inserted`
  (inside the transaction) or `published` (just after `commit`), and the
  parent kills it there. `inserted` leaves no journal and an `approved`
  plan; `published` leaves a journal carrying `plan_id` and the approval
  audit and an `executed` plan whose `journal_id` matches. Nothing else
  is possible between those two points.
- **Expiry gates first admission and nothing else.** `execute` returns the
  existing journal for an `executed` plan *before* `guard_admission` is
  reached, so a status read or a replay of a plan executed an hour ago is
  answered with its journal, never turned into an expired one, and
  `guard_admission` never writes. `load`/`require` do not expire. The
  ordering is asserted by the replay leg of
  `an_unknown_outcome_is_never_reposted_by_the_plan_path` and by the
  `now + EXPIRY` leg of `expired_invalidated_and_unapproved_plans_refuse_before_any_write`.
- **Every plan transition is guarded.** `prepared → approved`,
  `approved → executed`, `{prepared, approved, expired} → invalidated`
  and `{prepared, approved} → expired` are each a single
  `BEGIN IMMEDIATE` with a `WHERE … state IN (…)` clause, the same
  discipline §12.2 sets for the journal; an `executed` plan is history
  and no statement rewrites it. `approve` reports a zero-row plan update
  as a persistence failure (exit 13), which is what §12.2 says a lost
  expected-state guard means.
- **Refusals precede the network write, and the reason is right.** For an
  expired, invalidated or unapproved plan, execute refuses before the
  revalidation `GET`; the acceptance test asserts the mock server saw no
  non-`GET` request and that the journal table is empty. REPORT §3.2's
  three reasons map to exit 8 through `PlanError::refusal_reason`, and a
  missing plan reads as `invalidated`, never as approved :  the reading the
  brief asked for.
- **`plan_sha256` pins exact content.** The canonical document covers
  identity key and generation, consumer, course, assignment, kind, each
  file's name/size/sha256, the text `input_sha256`/`transform`/`sent_sha256`,
  the URL, a digest of the comment, the baseline, every observation and
  the admission window; it deliberately excludes the outbound bytes and
  the local paths, which are pinned instead by `sent_sha256` and the file
  hashes. The digest is taken over `serde_json::Value`, whose map is a
  `BTreeMap` (no `preserve_order` in the lock file), so key order does not
  depend on struct declaration order. §12.2 step 8's streamed-hash check
  is intact in `submit::execute`, and execute additionally re-hashes every
  frozen upload from disk before any journal exists :  changed bytes are
  `invalidated`, exit 8, with no upload. The digest tests cover a changed
  assignment, course, generation, baseline, window, consumer, comment,
  due date, `sent_sha256`, file hash and file name, and assert that moving
  the file or rewriting the outbound bytes alone does not change it.
- **Revalidation covers what §3.5 names.** `Observations` carries
  `can_submit`, `allowed_attempts`, `extra_attempts`, `group_category_id`,
  `submission_types`, `allowed_extensions`, `locked_for_user`, `due_at`,
  `lock_at` and `unlock_at` :  the brief's list plus the extensions and
  lock flag §3.5 also names :  read through exactly the accessors
  `check_eligibility` and `check_group_and_types` use, with the two list
  fields sorted so a reordered Canvas response is not a change.
  `every_changed_observation_invalidates_the_plan` walks all ten. The
  baseline attempt and submission id are compared separately, which also
  closes the case the brief does not ask about: two plans prepared for one
  assignment and both approved cannot both submit, because after the first
  commits the second's frozen baseline no longer matches.
- **No lock is held across human consideration.** `prepare` takes
  admission for its own pre-flight and drops it before returning, and a
  second process probes the lock and finds it free while a plan waits :
  with the probe proved real by a second run against a held lock.
- **The human `submit` keeps its v1 contract.** Exit codes, stderr
  confirmations and the single `submit@1` envelope are unchanged; the
  refusal envelope for the pre-existing paths still carries `details: {}`
  through `selected_error`, and only the new plan refusals add
  `{"reason": …}`, which is what REPORT §3.2's exit table asks for.
  `--yes` is recorded as `yes-flag`. The one behaviour change is the extra
  pre-flight `GET` §3.5 requires, and the three M2-b and two e2e snapshots
  that count requests record it (`api` 3 → 4, and 1 → 2 for the injected
  persistence failure, which now fails at the execute-side insert). The
  `course_code` fallback that `submit` used to patch in after pre-flight
  now travels through `PrepareRequest::course_code` into `facts_of`, so
  both the printed plan and the journal payload still carry it.
- **§15.** No new token handling, no new redaction surface, no raw
  response body on disk. `plan@1` is the one new document and its test
  asserts that the digests and file hashes travel while the outbound
  bytes, the comment text and the local file path do not :  the comment is
  reduced to `comment_chars`. The plan row itself lives in `state.sqlite`,
  which `open_db` creates `0600` inside the identity directory, the same
  containment the journal's intended payload already has.
- **Appendix D nullable convention.** `plan_id` and `approval` are
  `Option` with `#[serde(default)]` and no `skip_serializing_if`, so they
  serialize as `null` rather than vanishing, on `Journal`, the
  `receipts@1` journal summary and the receipt document alike;
  `an_unknown_outcome_is_never_reposted_by_the_plan_path` asserts a
  journal created without a plan keeps `plan_id = None`.
- **Migration.** `0002` is additive: two new tables, two `ALTER TABLE …
  ADD COLUMN`, one partial unique index, no rewrite of `0001`. The
  `user_version` gate is now a real list and my new tests cover the
  upgrade from a v1 database in both directions.

## Needs a decision

1. **`docs/SPEC.md` is behind the code.** Appendix D's `Journal` and
   `receipt@1` rows, and the §12.2 receipt example, do not carry
   `plan_id` and `approval`; REPORT §3.5 still calls them "the proposed
   Appendix D additions". The code, the registry fixtures and the e2e
   shape table now emit them. The worker could not fix this :  the brief
   forbids touching `docs/` :  and neither should a reviewer decide the
   spec. Rolf should add both fields to the two Appendix D rows and
   to the §12.2 receipt example.
2. **Plan retention.** `plans` rows hold the full outbound bytes of every
   prepared submission, including ones the person declined at the prompt,
   and nothing prunes them: `invalidate` and `expire` change the state and
   leave the payload. Until now `canvas submit` never persisted content it
   did not send. Neither §12.2 nor §3.5 gives a retention rule for a
   pre-journal artefact. Decide whether expired and invalidated plans are
   pruned, on what schedule, and whether `doctor` reports the backlog.
3. **The blocking wait on a contended admission lock.** §12.2 step 2 takes
   admission non-blocking and reports `in_progress` at once. `plan::execute`
   waits up to five seconds for the plan's own concurrent execute to
   publish its journal before reporting a conflict, because §3.5 requires
   a concurrent execute to return the existing journal rather than a
   refusal. The reading is safe :  it never creates a second journal, and a
   genuinely different submit still gets `in_progress` :  and §3.5 wins for
   the plan layer, but the wait is a behaviour the spec does not describe.
   Decide whether it belongs in §12.2 or whether the two cases should be
   told apart without blocking.
4. **No envelope for "this plan already has a journal".** §3.5 says a
   replayed acceptance returns the existing journal, and §3.2's exit table
   has no row for that outcome. The human `submit` currently reports
   `Admission::Existing` as exit 8 `refused` with no `reason`, which is
   unreachable in its single-flow interaction but is the ordinary answer
   for `submission.execute`. M6-b needs a defined outcome, reason and exit
   for it before the MCP adapter ships.
