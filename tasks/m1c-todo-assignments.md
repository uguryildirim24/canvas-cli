# M1-c — `todo`, `assignments`, `assignment`, `open` (Cursor Auto, lane w1)

Read `docs/SPEC.md` §5 (`todo`, `assignments`, `assignment`, `open`,
"Behaviour notes", "Command classes"), §6, §7, §10 (datasets
`assignments`, `submission`, `missing`, `planner`; hit predicate incl. the
window rule; "Pending hook"; request budgets), §12.1 (all of it), §14,
§16 rows 2–3, Appendix B, Appendix D (`SubmissionStatus`, `Availability`,
`Attachment`, `todo@1`, `assignments@1`, `assignment@1`, `open@1`).
Existing code: `canvas-api` models (planner items, missing submissions,
assignments, submissions), `canvas-core::store` (`Dataset`, `Supplied<T>`,
`field_obs`, hit predicate, `pending_for_assignment`), `canvas-core::sync`
and `canvas-core::resolve` (M1-b; the `<assignment>` resolution
signature), `canvas-core::journal` (M2-a; read-side owner probe,
supersession and acknowledge helpers), `canvas-core::markdown`,
`crates/canvas-cli/src/output` (envelope, registry, renderer base). Read
their public APIs first.

## Round-3 interface (do this first)
You are the **enum owner** and the **registry owner** this round. Before
anything else: make sure the command enum and dispatch have entries for
`files`, `modules` (M3-a, lane w2) and `submit`, `submission`,
`submission verify`, `submission reconcile`, `receipts list|show|export|
acknowledge` (M2-b, lane w3); a stub that exits 2 with `not implemented`
is enough. Add registry constants `files@1`, `modules@1`, `submit@1`,
`submission@1`, `receipt@1`, `receipts@1`, `verify@1`, `reconcile@1`,
each with a minimal fixture that satisfies the registry test (the owning
lane replaces it). Commit that alone, run the gates, and reply exactly
`DONE R3-interface`. Then continue below without waiting for an answer.

## Deliverables
1. Datasets in `canvas-core::sync`: `assignments(course)`
   (`include[]=submission`, all pages; `can_submit` is a tracked field with
   its own observation age per §10/§12.1), `submission(assignment)` (one
   object with `submission_history`, `submission_comments`,
   `rubric_assessment`), `missing` (`all`; `planner_overrides`, `course`),
   `planner(window)` (UTC-day window; coverage = the window; the hit
   predicate window rule). `Supplied<T>` three-state on tracked columns.
2. `canvas-core::todo`: kinds, keys, dates, merging, status fields,
   filters, and buckets exactly per §12.1. Buckets are one shared
   definition used by `assignments` too; `open` is defined once. `pending`
   comes from the §10 pending hook through the journal read helpers
   (read-only: never lock, never transition). Counts `missing`,
   `due_today`, `due_week`, `hidden`.
3. `canvas-core::resolve`: implement `<assignment>` resolution over the
   `assignments` dataset (numeric, URL with origin check, substring over a
   complete dataset, class-B never-fetch rule) and the `<url>` forms of
   `assignment`, `submit`, and `open` per §6.
4. Commands: `todo [--days N] [--all] [--missing] [--course <course>]`
   (baseline 3 requests: `courses:active`, planner window, `missing`;
   `--all` fetches `assignments` per active course, exit 7 offline);
   `assignments <course> [--bucket …] [--search TEXT]`; `assignment
   <course> <assignment>` and `assignment <url>` (prompt as Markdown via
   `canvas-core::markdown`; rubric assessment is one extra request only
   when a graded submission exists; `external_tool` prints the tool name
   and the `open` command); `open <course>`, `open assignment`, `open
   file`, `open announcement`, `open <url>` (class B: never fetches; a name
   resolves only over a complete cached dataset, else exit 6 with `use a
   numeric ID or a URL`; a foreign origin is exit 6; browser launch with
   the Appendix A crate; `launched` reports the result).
5. Renderers and schemas `todo@1`, `assignments@1`, `assignment@1`,
   `open@1` with fixtures; sorts per Appendix D; the human renderer shows
   `pending` items with an unknown status.
6. Tests (§16): todo merge cases (overdue graded quiz and graded
   discussion in both sources; dismissed missing work stays visible;
   unlocked assignment with `can_submit = false`; checkpoint and
   peer-review kinds; unknown kind retained); `--all` requires complete
   assignments (exit 7 offline); a full refresh that omits `can_submit`
   does not refresh its age; fresh `can_submit = false` gives
   `submittable = false`; baseline 3 requests on fixtures (the envelope's
   `requests`); every bucket incl. `open`; assignment resolver ambiguity,
   incompleteness, offline; `open` refuses a foreign origin and a name
   without a complete dataset; `pending = true` for a journal in every
   pending state and cleared after supersession and after acknowledge;
   snapshot tests for every schema in human and `--json` mode.

## Rules
- You own `crates/canvas-core/src/todo/**`, the four datasets above in
  `canvas-core::sync`, the assignment part of `canvas-core::resolve`, the
  `todo`/`assignments`/`assignment`/`open` command modules and renderers,
  and this round also the command enum, dispatch, and the schema registry.
- Do not change `canvas-core::journal` (lane w3 owns it). If you need a
  read helper there, add the smallest `pub fn` and name it in your final
  message.
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
Finish with `git status --short` and reply exactly: `DONE M1-c`
