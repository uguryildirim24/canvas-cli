# M4-a — Grades; `grades` (Cursor Auto, lane w1)

Read `docs/SPEC.md` §5 (`grades`, "Behaviour notes"), §7, §10 (datasets
`assignment_groups`, `enrollment_grades`, `course_totals`,
`grading_periods`; scope keys carry the period; request budgets for
`grades`), §12.4 (all of it; v2 local estimation is **out**), §14, §16
rows 2–3, Appendix B, Appendix D (`Grade`, `SubmissionStatus`, `grades@1`).
Existing code: `canvas-api` models (enrollments, assignment groups, grading
periods wrapper), `canvas-core::sync` (M1-b: `courses`, `terms`,
`enrollment_grades(period)`, `course_totals`, `grading_periods`),
`canvas-core::resolve`, `crates/canvas-cli/src/output` (envelope,
registry, renderer base). Read their public APIs first.

## Round-4 interface (do this first)
You are the **registry owner** this round. Before anything else, add the
registry constants `grades@1`, `download@1`, `announcements@1`,
`announcement@1`, `calendar@1` (each with a minimal fixture that satisfies
the registry test; the owning lane replaces it), commit that alone, run
the gates, and reply exactly `DONE R4-registry`. Then continue below
without waiting for an answer. Lane w3 (M4-b) owns the command enum this
round; keep your dispatch arm in your own module and `git merge main` when
Claude tells you the enum landed.

## Deliverables
1. Dataset `assignment_groups(course, period)` in `canvas-core::sync`
   (Appendix B request, `grading_period_id` when a period is selected; all
   pages; `Supplied<T>` on tracked columns) and the period-aware reads over
   `enrollment_grades`, `course_totals`, `grading_periods` from M1-b.
2. `grades [<course>] [--period current|all|ID]` exactly per §12.4: the
   default mode depends on whether the course has grading periods;
   `--period ID` validates the ID against `grading_periods` and reports
   `unavailable` for courses without that period; course view with groups
   (`name`, `group_weight`, `rules`), assignments with the listed fields,
   subtotals only when the API supplies them, Canvas totals for the
   selected mode closing the table; absent or `null` totals print
   `unavailable` and stay `null`; totals from one mode are never labelled
   with another. Baseline requests per §10: 2 for the overview, +1 per
   course for the course view, +1 grading periods, +1 enrollments per
   explicit period.
3. Renderer and schema `grades@1` with fixtures; sorts per Appendix D.
4. Tests (§16): period modes (no periods → `all`; periods → `current`;
   explicit ID valid, invalid, and absent for one course); grading-periods
   wrapper pagination across two pages; `null` totals; duplicate
   enrollments de-duplicated per course; scoped enrollment values P→Q→P
   incl. out-of-order observations (composite observation keys);
   `--offline` with and without coverage; snapshot tests in human and
   `--json` mode; request counts on fixtures.

## Rules
- You own the `assignment_groups` dataset, the period-aware read helpers,
  the `grades` command module and renderer, and the schema registry this
  round. Do not touch the command enum (lane w3 owns it this round).
- Work on branch `lane/w1` in this checkout. Commit as you go. Before
  reporting, `git merge main` (resolve, rerun gates). Do not push.
- Do not touch `docs/` or `tasks/`.

## Gates
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
```
Finish with `git status --short` and reply exactly: `DONE M4-a`
