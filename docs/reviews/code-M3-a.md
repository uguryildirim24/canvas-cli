MERGE
Identified M3-a defects are fixed in seven review commits on `lane/w2`; no unresolved specification decisions.
All five required gates pass on code commit `72693656398892275b6fb2c9abe84c007b91ee96`, including 295 tests and Rust 1.88.0.

## Scope and verification

Reviewed `tasks/m3a-files-modules.md`, its cited SPEC sections, the store/API/planner/output interfaces, `git log main..HEAD --stat`, and the full package diff. The requested initial `git merge main` returned `Already up to date`; the worker baseline was `da7c486f8782b0a4fc1d4795689f1b27db7da402`. No subsequent merge or push was performed.

Every gate used `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/rev-m3a`.

| Gate | Initial result | Final result |
|---|---|---|
| `cargo fmt --all --check` | PASS | PASS, exit 0 |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS | PASS, exit 0 |
| `cargo nextest run --all-features` | PASS, 283 tests | PASS, 295 tests across 20 binaries; 0 skipped; exit 0 |
| `cargo deny check` | PASS | PASS, exit 0; advisories, bans, licenses, sources OK; 14 non-fatal duplicate-version warnings |
| `cargo +1.88 check --workspace --all-targets` | PASS | PASS, exit 0; `rustc 1.88.0 (6b00bc388 2025-06-23)` |

The final nextest run ID was `5084c330-c03a-4fe6-81fa-2529a25b721a`. Targeted core and CLI regression runs also passed. Verification used local SQLite stores, wiremock HTTP endpoints, real CLI subprocesses, and existing human/JSON snapshots; no live Canvas account was queried.

## Defects found and fixed

Locations below refer to the reviewed worker baseline `da7c486`, before fixes. Paths are relative to the repository root.

| Severity | File:line | What was wrong | What changed | Fix commit |
|---|---|---|---|---|
| High | `crates/canvas-core/src/sync/modules.rs:343` | Item ingestion only upserted entity rows. Removed items remained visible to both commands and download discovery indefinitely. | Replace per-module item membership atomically, retain unreferenced entities, and constrain all three readers to current membership. An older item response cannot resurrect membership. | `66b26d3` |
| Medium | `crates/canvas-core/src/sync/refresh.rs:221`; `crates/canvas-core/src/sync/modules.rs:198` | Converting observed items back to models discarded field presence; item `html_url` and completion values were never ingested. Explicit null access/content fields could not clear older values. | Preserve observations for inline and separately fetched items; ingest allowlisted URL/completion values and nested nulls with individual clocks. Preserve supplied module state, including null. | `3ea7482` |
| Medium | `crates/canvas-core/src/sync/wire.rs:234` | Raw-value overrides undid timestamp normalization; several folder/module detail nulls were ignored. The `content-type` alias also lost explicit nulls. | Retain normalized typed values and add explicit-null writes for all retained tracked fields, including the content-type alias. | `3ea7482`, `690960a` |
| High | `crates/canvas-core/src/sync/files.rs:148` | Discovery persisted signed download/thumbnail URLs, and folder endpoint URLs could contain credentials. | Omit unneeded file/folder URLs from persistence. Strip credentials, query, and fragment from module navigation URLs. Added a persistence regression containing signed URLs and untracked nested response fields. | `c8d86bd` |
| High | `crates/canvas-core/src/store/dataset.rs:135` | Allowing complete denials also allowed every other complete/error combination to replace membership, weakening the shared failure contract. | Permit error-bearing complete coverage only for files/folders with an exact recorded 403/404 denial; all other errors retain previous rows and mark them stale. | `95832e7` |
| Medium | `crates/canvas-core/src/sync/refresh.rs:487` | A failed retry overwrote a retained listing denial, so later reads could claim the empty listing was available. | Preserve the recorded availability denial while marking it stale, both in the returned outcome and stored fetch log. | `100bf7d` |
| Medium | `crates/canvas-cli/src/commands/files.rs:108` | Folder denials disappeared from `partial[]`; file denials returned exit 0 despite SPEC §14's partial exit 12. | Report folders/files independently, preserve the required file-denial message, and return the files schema with partial outcome and exit 12 online and offline. | `100bf7d` |
| Medium | `crates/canvas-cli/src/commands/files.rs:285` | An unknown/null effective `locked_for_user` value fell back to the separate generic `locked` flag. | Keep effective access unknown when unknown; hidden visibility remains independent. | `7269365` |
| Low | `crates/canvas-cli/src/commands/files.rs:474` | Numeric Canvas IDs were compared as decimal strings, putting ID 10 before ID 2 in otherwise equal rows. | Compare IDs numerically for file ties and module-position ties. | `7269365` |
| Medium | `crates/canvas-cli/src/commands/files.rs:505` | `--tree` printed complete folder paths with one fixed indentation level, losing nested hierarchy. | Print shared ancestors once and indent each folder/file by its actual depth. | `7269365` |
| Medium | `crates/canvas-core/src/sync/files_modules_tests.rs:72`; `crates/canvas-core/src/sync/files_modules_tests.rs:124` | Required coverage was mostly predicate-level: no real request-count test for all four inline cases, no HTTP 404/cache-reuse test, and no actual CLI HTTP 401 assertion. | Added HTTP pagination/fallback counts, all three listing pagination checks, failed-item atomicity, 403/404 cache/offline/retry cases, throttle precedence with five attempts, actual CLI 401/telemetry, and independent folder-denial coverage. | `3ea7482`, `100bf7d`, `690960a` |

## Contract and security checks

The `files@1` and `modules@1` fixtures and human/JSON snapshots pass. Regression coverage verifies separate hidden/effective-lock values, search case handling and misses, nested trees, recorded-denial offline behavior, per-field null/absence/out-of-order handling, and removal of stale module-item references.

The planner adapter remains `canvas_core::sync::discovery_plan_input`, producing the existing `download::PlanInput` types for `download::plan_course`. Listing files, folders, modules, and item membership feed the existing planner's merged-file and ownership logic. No planner type change or database migration was needed; item membership uses the existing generic membership table.

Dependency manifests and `Cargo.lock` were not changed. Appendix A pins, including keyring `=4.2.0`, remain intact. Discovery persists allowlisted records, with no raw response bodies or transfer capabilities in the added regression. Existing API origin/redaction and download containment tests pass in the full suite; this package does not implement download execution.

## Needs a decision

None.
