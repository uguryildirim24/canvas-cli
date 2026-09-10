# M3-a — Files, folders, modules ingestion; `files`, `modules` (Cursor Auto, lane w2)

Read `docs/SPEC.md` §5 (`files`, `modules`, "Behaviour notes"), §7, §10
(datasets `modules`, `folders`, `files`; "complete when all pages, or a
recorded denial"; hit predicate), §11 (error classification), §12.3
("Discovery", "Denial classification", "Coverage claim"; the `download`
command itself is M3-b in round 4), §14, §16 rows 2–3, Appendix B,
Appendix D (`files@1`, `modules@1`). Existing code: `canvas-api` models
(folders, files, modules, module items, `content_details`),
`canvas-core::store` (`Dataset`, `field_obs`, `fetch_log`),
`canvas-core::sync` (M1-b datasets), `canvas-core::download` planning
inputs (M3-b-core: what the planner expects from discovery),
`crates/canvas-cli/src/output`. Read their public APIs first.

## Deliverables
1. Datasets in `canvas-core::sync`: `folders(course)` and `files(course)`
   (all pages; classification in order: throttle first, `401` → exit 3,
   `403`/`404` → the listing is recorded as a denial `unavailable` with the
   HTTP status, the dataset counts as complete, and the hit predicate
   serves the denial within TTL); `modules(course)` (`include[]=items&
   include[]=content_details`, then `GET …/modules/:mid/items?include[]=
   content_details` for every module whose inline `items` is absent,
   `null`, or shorter than `items_count`; per-module `items_complete`
   recorded). Module `File` items keep `content_id`,
   `content_details.locked_for_user`, `lock_explanation`, and the module
   `state`. `hidden` (listing visibility) and `locked` (effective access)
   are separate fields and are never merged.
2. A discovery view for the download planner (M3-b-core): the merged file
   set from the listing and from module items with `source: listing |
   module`, folder path, module id and position, in the types the planner
   expects. If the types differ, add the smallest adapter and name it in
   your final message.
3. Commands: `files <course> [--tree] [--search TEXT]` (a denial prints
   `Files listing unavailable (HTTP 403); showing files linked from
   modules` and appears in `partial[]`), `modules <course> [--items]`.
   Renderers per §7; schemas `files@1`, `modules@1` with fixtures; sorts
   per Appendix D.
4. Tests (§16): inline-item completeness rule (absent, `null`, shorter
   than `items_count`, complete) with request counts; denial
   classification (`401` exit 3; `403`/`404` recorded and served from cache
   within TTL; throttle precedence); `hidden` vs `locked` never merged;
   `--tree` and `--search`; `--offline` with a recorded denial; snapshot
   tests for both schemas in human and `--json` mode.

## Rules
- You own the three datasets above in `canvas-core::sync`, the discovery
  view, and the `files`/`modules` command modules and renderers. M1-c
  (lane w1) owns the command enum and the schema registry this round and
  adds your variants and `files@1`/`modules@1` first; `git merge main`
  when Claude tells you that interface landed. Until then keep your
  dispatch arms in your own module.
- You are the **migration owner** this round; none is expected. If you
  must add one, number it after the last existing migration and say so
  in your final message.
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
Finish with `git status --short` and reply exactly: `DONE M3-a`
