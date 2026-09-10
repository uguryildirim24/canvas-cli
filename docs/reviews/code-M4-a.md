# Code review — M4-a (`grades`), branch `lane/w1`

Reviewer: Claude Opus 5 (high). Base: `e1e78f3` (merge of `main` into
`lane/w1`). Package brief: `tasks/m4a-grades.md`. Spec: `docs/SPEC.md`
§5, §7, §10, §12.4, §14, §15, §16 rows 2–3, Appendix A, B, D.

## Verdict

**MERGE-AFTER-DECISION.** The dataset, the command, the schema and the
period scoping match §10 and §12.4, and all five gates are green after
six `review(M4-a):` commits that fix six defects — one of them a spec
violation that mislabelled whole-course totals as the current period.
Three open questions remain, all about behaviour §12.4 does not define
for the no-operand overview and for `--period current`; none of them
blocks a merge, and none can be settled without an owner decision.

## Gate results

Run with `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m4a`
at `fd7f826`.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass |
| `cargo nextest run --all-features` | pass — 388 tests run, 388 passed (384 at the base) |
| `cargo deny check` | pass — advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | pass |

The package adds no dependency, so Appendix A is unchanged.

## Defects found and fixed

| # | Sev | File:line (at `e1e78f3`) | What was wrong | What I changed | Commit |
|---|---|---|---|---|---|
| 1 | High | `crates/canvas-cli/src/commands/grades.rs:260` | `totals_for` fell back to the unqualified `enrollment_grades` (`period:none`) rows whenever `course_totals` had no row for the course, **including under `--period current`**. Those rows carry Canvas' whole-course values, so a course the course list did not cover reported its `all` total under `period.mode = "current"`. SPEC §12.4: "Totals from one period mode are never labelled with another." | Restricted the fallback to `--period all`, where the unqualified values *are* the whole-course ones. `--period current` now reports `unavailable`. Seeded a third fixture course (PHYS-301) with no `course_totals` row and two `none` enrollments, and added `an_uncovered_course_never_borrows_whole_course_totals_for_current` asserting both directions. | `5e6685d` |
| 2 | Medium | `crates/canvas-core/src/sync/assignment_groups.rs:175` | `assignment_projection` always emitted the nine submission-derived keys, filling them with `null` when Canvas answered the assignment with no `submission` object at all. The course-view read derives `submitted` from the presence of `submitted_at`, so an assignment Canvas said nothing about was reported as `submitted: false` while every sibling status field stayed null. SPEC §10 separates absent from null; Appendix D's `SubmissionStatus.submitted?` uses null for unknown. | Build the projection as a map and add the status keys only when a `submission` object was supplied. Added `an_assignment_without_a_submission_carries_no_status_keys`, which checks both shapes in one refresh. | `d9293b6` |
| 3 | Medium | `crates/canvas-cli/src/commands/grades.rs:456` | SPEC §12.4 defines the course view as "groups (`name`, `group_weight`, `rules`), assignments …; group subtotals only when the API supplies them; **Canvas course totals for the selected period mode close the table**". The renderer printed the totals table *first* and never printed the drop rules, so two clauses of that sentence did not hold. | Split the overview table into `write_course_table` and reused it. The course view prints the course line, the groups, then the totals table closing it; the overview is unchanged. Group drop rules print under the group name (`drop lowest 1 · never drop 31`); zero counts read as no rule. Snapshot refreshed. | `0582b2a` |
| 4 | Medium | `crates/canvas-cli/src/commands/grades.rs:116` | The envelope declared `courses`, `enrollment_grades`, `grading_periods` and `assignment_groups`, but not `course_totals` — the dataset the two default modes actually read. It has its own TTL and its own per-field observation clocks (§10), so a reader could not tell whether the totals on screen were stale, which is what §7's `freshness` array is for. `courses` and `course` already declare it. | Gave `grade_freshness` an explicit mode parameter so a caller can name the `course_totals` mode it read rather than the course's default (the three existing callers pass `None` and keep their behaviour), and extended the grades envelope with those rows for `all` and `current`. An explicit period reads no `course_totals` row and adds none. | `fd7f826` |
| 5 | Medium (missing test) | `crates/canvas-core/src/sync/grades_tests.rs:344` | §16 names "scoped enrollment values P→Q→cached P incl. **out-of-order P (t=30) then Q (t=20)** observations on the same enrollment". The test walked P(t=10), Q(t=20), P(t=30) — three arrivals whose clocks only ever increase, so the composite observation key was never asked to keep an older Q write from being suppressed by P's newer clock. The brief lists this case, so its absence is a defect. | Added `a_newer_observation_for_one_period_never_suppresses_an_older_one_for_another`: P at t=30 first, then Q at t=20, asserting Q keeps both its value and its own older `observed_at`. | `84270ec` |
| 6 | Low (weak test) | `crates/canvas-cli/tests/grades.rs:498` | `duplicate_enrollments_collapse_to_one_row_per_course` only counted result rows. Those rows come from the course list, not from the enrollments, so the de-duplication rule the test named could never fail it — every course seeded also had a covering `course_totals` row, so `enrollment_grades_by_course` was never consulted. | The PHYS-301 fixture from defect 1 has no `course_totals` row and two `none` enrollments, one empty and one with values. The test now asserts the valued enrollment wins (64.0 / "D" / 60.0). | `5e6685d` |
| 7 | Low | `crates/canvas-cli/src/output/registry.rs:651`, `schemas/grades.json:88` | This lane owns the schema registry this round. `download@1` carries a test proving every Appendix D field serializes even when nothing is known; `grades@1` had no equivalent, so a future `skip_serializing_if` could silently drop keys. The fixture also claimed `graded: null` beside `workflow_state: "unsubmitted"` — a pair the renderer cannot produce, since `graded` is derived from the workflow state whenever Canvas supplies it. | Added `grades_optional_fields_are_always_present_and_nullable`: a fully unknown `SubmissionStatusJson` serializes all thirteen keys; the fixture carries every `Grade`, status and optional key; the fixture round-trips through `GradesResult` unchanged. Corrected the fixture to `graded: false`. | `37055cb` |

## What I checked and found correct

- **Appendix B requests.** `assignment_groups_path` and `enrollment_grades_path`
  match the `grades` row exactly, including `include[]=assignments`,
  `include[]=submission`, `override_assignment_dates=true`, `per_page=100`
  and the optional `grading_period_id`.
- **§10 scoping.** Dataset scope `course:<id>:period:<id|none>`; the group
  entity's own columns share one clock under the plain group id while the
  per-period assignment list lives in `data_json.assignments_by_period`
  with its own `observed_at`, so a period-P fetch cannot overwrite or
  freshen period Q's list. `enrollment_grades` keys stay `<id>|<period>`
  and `course_totals` `<id>|<mode>`.
- **Mutation epochs.** `AssignmentGroupsDataset::epoch_scope()` is
  `assignment_groups:course:<id>:period:<p>`, which the journal's
  `assignment_groups:course:<id>:*` prefix bump matches
  (`crates/canvas-core/src/journal/ops.rs:787`). `cache clear` includes
  the table.
- **Request budgets (§10).** Verified on fixtures: overview 2, course view
  4 (courses + enrollments + grading periods + one group fetch), explicit
  period 4 (courses + grading periods + period enrollments + period
  groups), rerun inside the TTL 0.
- **Exit codes (§14).** `--period midterm` → 2; an id no course reports
  grades for → 6; `--offline` without coverage → 7, naming the missing
  dataset rather than the courses cache; no identity → 3. A period the
  selected course lacks is a value (`unavailable`), not an error, per
  §12.4.
- **Security (§15).** The stored projections are allowlists — no
  `html_url`, no signed or capability-bearing URL reaches
  `assignment_groups.data_json`, and a core test asserts it. No raw body
  is persisted. The budget suite asserts the token never appears in
  stdout.
- **Appendix D `grades@1`.** Field names, types, nullability and the three
  sorts (courses by `code` then `id`, groups by `position`, assignments by
  `due_at` then `id`, undated last) all match.
- **Registry entries owned for lanes w2/w3.** `download@1` carries all four
  once-skipped fields as always-present nullables and all thirteen
  `totals` counters, matching Appendix D. The `announcements@1`,
  `announcement@1` and `calendar@1` placeholders match their Appendix D
  shapes (`announcement@1` carries the full announcements item plus
  `message_markdown`); they are minimal but correct for the owning lanes
  to replace.
- **MSRV and dependencies.** No `Cargo.toml`, `Cargo.lock` or `deny.toml`
  change; `cargo +1.88` is clean.

## Needs a decision

1. **`--period ID` with no course operand never reads `grading_periods`.**
   §12.4 says the `grading_periods` dataset "validates the ID and supplies
   the title", but that dataset is scoped `course:<id>`, so the overview
   has no single course to read it from. The implementation instead treats
   an id as unknown only when Canvas reported no grades for it anywhere
   (exit 6) and leaves `period.title` null for every course. Fetching
   periods for every active course would break the §10 budget; the
   alternatives are to define the overview's `--period ID` title as
   always null, or to require a course operand with an explicit id.
2. **`--period current` fetches assignment groups for `period:none`.**
   §12.4 pins the group fetch to the ID only for `--period ID`. Under
   `--period current` the course view therefore lists every assignment in
   the course while closing with the current-period total. That is a
   defensible reading of a silent spec, but if the intent is that the
   course view shows the selected period's assignments, `current` should
   resolve to the course's current grading period id and fetch with it —
   which changes the request budget and the cache scope, so it is a spec
   change, not a code fix.
3. **The overview's default mode when courses disagree.** §12.4 makes the
   default depend on "whether the course has grading periods", but
   `grades@1` carries one `period_mode` for the whole run. The
   implementation picks `current` when *any* listed course has grading
   periods, so courses without them then report `unavailable` instead of
   their whole-course grade. The alternatives — `all` unless *every*
   course has periods, or a per-course mode — both need an Appendix D
   decision.

## Observations (no change made)

- `assignment_groups.data_json.assignments_by_period` is a parallel store
  for assignment and submission values that §10 also models in the
  `assignments` and `submissions` entity tables. A grades fetch supplies
  `points_possible`, `due_at`, `score` and `workflow_state`, but writes
  none of them there, so `assignments <course>` and `grades <course>` can
  show different values for the same assignment and neither refreshes the
  other's field clocks. §10 does not say where per-group per-period
  assignments belong, and unifying them is a refactor well beyond a
  review fix; flagging it for the round-5 owner.
- `GroupSubtotalJson` is read from `assignments_by_period[…].subtotal`,
  which the ingest never writes, because the `assignment_groups` endpoint
  supplies no subtotal. Always-null is what §12.4 asks for ("only when
  the API supplies them"), but the schema fixture and the CLI fixture
  both seed a subtotal the real ingest cannot produce.
- `load_grading_periods` treats a period with neither `start_date` nor
  `end_date` as containing `now`, so several such periods would all
  report `is_current: true`. Canvas requires both dates on a grading
  period, so this is theoretical.
