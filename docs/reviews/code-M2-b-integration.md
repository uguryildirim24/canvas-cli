MERGE  
The M1-c integration this follow-up covers is correct against SPEC v0.8; the two defects below are fixed and committed on `lane/w3`.  
All five gates pass on the reviewed tree; no push and no merge were performed.

## Scope and gate results

Reviewed `tasks/m2b-submit-receipts.md` parts 5–7, the SPEC sections it cites, `docs/reviews/code-M2-b.md`, and the diff since that review's last commit `c6f3258`. The integration commits are `d0fb793` (submit operand resolution), `efa9ad2` (`submission` show over the M1-c dataset, plus the crash-test handshake and the upload-concurrency assertion), `209c8fc` (integration acceptance tests) and `aa0128e` (the `download@1` nullability step). The requested `git merge main` is `bd3a4d7` (main parent `61284b3`); it was a trivial docs-only merge. Final revision `f2f0acc`. Main advanced independently to `558134e` with M4-a during this review; these results describe this worktree, not a merge with that newer main.

Every gate used `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m2b-int`.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo nextest run --all-features` | PASS: 396 passed, 0 skipped, 24 binaries. Run `086c5bf0-2f4d-4290-8df2-9e84fe6c7b60`. |
| `cargo deny check` | PASS: advisories, bans, licenses, sources all OK. |
| `cargo +1.88 check --workspace --all-targets` | PASS |

The same five gates also passed on the worker's tree before any fix (396 tests). `git diff --check` passes. Appendix A pins are untouched; no dependency was added or moved.

## What was verified

- **Operand resolution (part 5, `d0fb793`).** Pre-flight step 1 runs the §6 resolver before the assignment `GET`, the admission lock and any journal, so a resolution failure still leaves no journal. Course by numeric id, alias, URL and substring; assignment by numeric id, URL and name substring inside the resolved course. The §6 origin check and the course-agreement check both refuse with exit 6, and a resolution timeout still classifies as exit 4 per §12.2 step 1. The resolved course code is written into the plan **before** `create_from_plan`, so the journal row, the receipt and a later `receipts export` rebuild all carry it. `submission` is held to exactly two operands by the existing clap check, matching §5.
- **`submission <course> <assignment> [--history]` (part 5, `efa9ad2`).** The `submission@1` result carries every Appendix D field, in `SubmissionStatus` order, with IDs as decimal strings, `attempt`/`score` as numbers, arrays never null, `rubric_assessed` paired with `rubric_assessment`, and `history` sorted by `attempt` ascending and present as `[]` without `--history`. The §10 pending hook nulls exactly the nullable `SubmissionStatus` fields and keeps `missing`, which is the masking `canvas-core::todo` already applies; the observed payload around the status is still reported. `submitted`/`graded` are derived from `workflow_state` with the same rule `sync::assignments::push_submission_status` uses. Only digests reach disk, and the attachment projection carries no capability URL (§15).
- **Readers during active phases (part 7, `209c8fc`).** `todo`, `submission`, `receipts list` and `submission reconcile` each run as a real second process against a journal held live in `planned`, `uploading`, `uploaded` and `posting`, and again against a real `submit` waiting on its `POST`. Each reports `pending`, names the journal, reports `owner: live`, spends zero API requests in `reconcile`, and leaves the state unchanged. Both the JSON and the human renderer paths are exercised.
- **Fixtures (part 6).** The `submission@1` registry fixture is now asserted equal to the command's own `--json` result, and `submit@1`, `verify@1`, `reconcile@1`, `receipts@1` and `receipt@1` are asserted field-for-field against the live commands, so no placeholder survives.
- **The two changes outside the integration.** The crash-test handshake now publishes by write-then-rename; the old plain `write` let the parent read a created-but-empty readiness file. The upload-concurrency assertion now waits for the invariant under a bound instead of a fixed 80 ms sleep. Both are real fixes and both were verified in the diff.

## Defects found and fixed

Locations refer to the corrected source. Paths are relative to the repository root.

| Severity | File:line | What was wrong | What changed | Commit |
|---|---|---|---|---|
| Medium | `crates/canvas-core/src/download/install.rs:696`; `crates/canvas-core/src/download/contain.rs:228` | The test `scratch()` helpers built temporary directory names from `SystemTime::now().as_nanos()` plus a process-local counter, in the shared system temp directory. Neither part makes the name unique across processes: the counter restarts at zero in every process, and the clock has 1 µs granularity on this machine. Two of the test processes nextest runs in parallel that reach the helper in the same microsecond therefore get the same path, `create_dir_all` succeeds for both, and they share one directory. The directories were also never removed. | Added `canvas_core::test_scratch::Scratch`: the process id makes the name unique by construction, and the tree is removed on drop. `install.rs`, `contain.rs` and `manifest.rs` now use it. | `152fddf` |
| Low | `crates/canvas-cli/src/commands/submission.rs:679` | The `submission` human renderer printed raw `_local` strings (`2026-09-09T12:05:00-04:00`) for the submission time, each comment and each history row. §7 fixes one human date format for the class-C reads, and `todo`, `assignments` and `assignment` already use it. | Format those three in the identity zone as `Wed Sep 9, 12:05 PM`. Added `output::format_local_instant` for the absolute half of the §7 date; `format_local_datetime` is now defined in terms of it and is unchanged for due dates. | `f2f0acc` |

### Evidence for the first defect

The worker reported that `download::install::tests::pending_move_recovery_branches` failed once in about 20 full runs and could not be reproduced in isolation. The claim is real and measurable:

- `SystemTime::now().as_nanos()` on this machine advances in steps of exactly 1000 ns; 96% of consecutive calls return the identical value.
- Of the 1763 directory names those two helpers had left in the temp directory, every one is a multiple of 1000 ns and the smallest gap between two distinct names is exactly 1000 ns — one clock tick. A pair that lands in the same tick leaves a single shared directory and no second name, so the listing cannot show a collision directly; it can only show that the margin is one tick.
- `pending_move_recovery_branches` asserts an exact recovery list (`assert_eq!(recovered, vec![(1, Action::Moved)])`), so a second process writing into the same directory adds rows and fails it, while the test alone is deterministic. That matches the reported symptom exactly.

After the fix a full `--all-features` run leaves zero directories behind from these helpers. The 5805 directories from earlier runs were left in place rather than deleted.

## Observation for another lane (not counted against M2-b)

`sync::refresh::ingest_success` turns an epoch abort into `SyncError::Db`, which `classification()` maps to exit 13. §10 says such a refresh "aborts (leaving the previous rows)", which reads as: abandon the refresh and serve what is cached, not fail the command. A first-ever `canvas submission <course> <assignment>` issued while a `submit` for that assignment is mid-flight bumps `submission:assignment:<id>` under the reader's refresh and exits 13. The path is in `crates/canvas-core/src/sync/refresh.rs:743` and `crates/canvas-core/src/store/dataset.rs:261`, shared by every class-C command and owned by M1-a/M1-c, so it is left for that lane rather than changed mid-round. The M2-b reader tests do not hit it because they hold the journal in a steady state.

## Needs a decision

- §7 gives one human date form, `Tue Sep 15, 11:59 PM` "plus `(in 2d 4h)` or `(overdue 3h)`". The suffix reads a date as a deadline, so on a time something happened at — a submission time, a comment time, a history row — it would print "overdue 3h" for work submitted three hours ago. The fix above renders the absolute half only and adds `format_local_instant` for it. If the owner wants the suffix everywhere, §7 should say so and the helper can be dropped; if not, §7 should say the suffix applies to due dates.
- The M2-b `receipts`, `verify` and `reconcile` renderers print raw `_local` strings for the same kind of value. They were reviewed as MERGE in `docs/reviews/code-M2-b.md` and are outside this follow-up's diff, so they were left alone. Whether they should follow the §7 form as well is the same decision.
