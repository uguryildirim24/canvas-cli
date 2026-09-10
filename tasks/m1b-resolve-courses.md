# M1-b — Resolvers, `courses`, `course`, `alias *`, `sync` (partial), output layer (Cursor Auto, lane w2)

Read `docs/SPEC.md` §5 (`courses`, `course`, `sync`, `alias`), §6, §7 (all),
§10 (datasets table: `courses`, `enrollment_grades`, `course_totals`,
`grading_periods`; hit predicate; request budgets), §12.4 (what `courses`
carries for grades), §14, §16 row 3, Appendix B, Appendix D (`Course`,
`Grade`, `Freshness`, `courses@1`, `course@1`, `alias@1`, `sync@1`,
`cache@1`, `error@1`). Existing code: `canvas-api` models and client,
`canvas-core::store` (`Dataset` trait, `field_obs`, membership, hit
predicate, epochs). Read their public APIs first.

## Deliverables
1. `crates/canvas-cli/src/output/`: the **JSON envelope** (§7: `schema`,
   `generated_at`, `profile`, `identity`, `freshness[]`, `requests`,
   `partial[]`, `warnings[]`, `outcome`, `exit`, `result`), the **schema
   registry** (`canvas-cli/<command>@<n>` constants + a test that every
   registered schema has a fixture), the raw-output rule, single-document
   rule, `--json` disables color/progress, and the human renderer base
   (`comfy-table` borderless, date formatting in the identity zone with
   relative suffixes, status labels/colors per §7, `NO_COLOR`/
   `CLICOLOR_FORCE`). Provide `Envelope::new(schema, profile, identity)`.
2. Concrete datasets in `canvas-core::sync`: `courses` (`active` | `all`,
   three requests for `all`), `terms`, `enrollment_grades(period)`,
   `course_totals(course_id, mode)` derived from `courses`,
   `grading_periods` (wrapped, paginated). Each implements the `Dataset`
   trait with `Supplied<T>` three-state fields for tracked columns.
3. `canvas-core::resolve`: `<course>` resolution per §6 (numeric, alias,
   URL with origin check, substring over complete memberships, class-B
   never-fetch rule), `<assignment>` resolution signature (over the
   `assignments` dataset that M1-c adds; implement against the trait now).
4. Commands: `courses [--all] [--term] [--favorites]`, `course <course>`,
   `alias set|list|remove`, `sync` (refreshes the datasets above; `--full`
   is assembled in M4-b), `cache stats|clear|path` renderers on top of the
   core functions from M1-a.
5. `--fresh`, `--offline`, TTLs, `stale: true` reporting, exit 7 rule.
6. Tests: resolver ambiguity/incompleteness/offline; `courses` fresh vs
   cached vs stale (failed refresh serves stale), `--all` three requests,
   `count = 0` complete dataset succeeds offline; envelope snapshot tests
   with `insta` (`COLUMNS=100 --color never`, fixed `TZ`, frozen
   `CANVAS_NOW`); every registered schema rendered once; exit-code precedence
   for aborts 2/3/7.

## Rules
- You own `crates/canvas-cli/src/output/**`, the `courses`/`course`/
  `alias`/`sync`/`cache` command modules, `canvas-core::resolve`,
  `canvas-core::sync`. M0-c (lane w1) owns the command enum/dispatch this
  round: add your dispatch arms in your own module and keep the edit to
  `main.rs` minimal; expect to `git merge main` and resolve.
- Work on branch `lane/w2` in this worktree. Commit as you go. Before
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
Finish with `git status --short` and reply exactly: `DONE M1-b`
