MERGE
M1-c's identified defects are fixed and committed on `lane/w1`; the round-3 interfaces are ready for their owning lanes.
All five gates pass, including 290 tests; no specification decision is required.

## Scope and verification

Reviewed `tasks/m1c-todo-assignments.md`, `tasks/m2b-submit-receipts.md` part 5, the cited SPEC sections and appendices, the public store/resolver/journal APIs, `git log main..HEAD --stat`, and the worker changes. The requested initial `git merge main` completed as `721df12`. The implementation reviewed was `5d17fab`; the final code revision is `bacac97`. No push or subsequent merge was performed.

Verification uses local unit tests, wiremock HTTP fixtures, separate CLI processes, and human/JSON snapshots. It does not claim live Canvas-account validation. Browser success/failure rendering uses an injected launcher in a unit test; foreign-origin refusal is additionally exercised through the CLI without launching a browser.

## Gates

All Cargo commands used `CARGO_TARGET_DIR=<checkout>`.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo nextest run --all-features` | PASS: 290 passed, 0 skipped, across 19 binaries. Final run `08ca416e-beeb-4787-98a3-bcafe492aa08`; output verbosity flags only were added on the final rerun. |
| `cargo deny check` | PASS: advisories, bans, licenses, and sources. Existing duplicate-dependency warnings remain nonblocking under `deny.toml`. |
| `cargo +1.88 check --workspace --all-targets` | PASS |

Dependency declarations and `Cargo.lock` are unchanged. Appendix A's exact pins remain in place, including `open = 5.4.3`; the existing Markdown implementation uses `htmd = 0.5.5`.

## Defects found and corrected

Locations below refer to the corrected code. Severity describes the original behavior.

| Severity | File:line | What was wrong | What changed | Commit hash |
|---|---|---|---|---|
| High | `crates/canvas-cli/src/commands/open.rs:139` | A string-prefix origin check accepted lookalike foreign hosts and userinfo URLs. | Reused structural origin validation; reject differing schemes, hosts, ports, and userinfo. Added adversarial URL tests. | `74d086c` |
| High | `crates/canvas-cli/src/commands/assignment.rs:11` | Detail responses were discarded, a numeric target fetched the whole list, network errors were ignored, and output invented submission status and empty feedback. | Added separate single-assignment coverage, persisted its response, propagated failures, and rendered actual dates, eligibility, prompt, submission, rubric, and tool metadata. Fetch feedback only for a graded submission. | `9edf811`, `e7a89ea` |
| High | `crates/canvas-cli/src/commands/assignment_read.rs:15` | Offline paths skipped coverage checks; reads could succeed with no required data. Network reads bypassed the session's identity-token validation. | Centralized complete-cache lookup, offline miss 7, cache freshness, lazy token validation, and request/error telemetry. | `e7a89ea` |
| High | `crates/canvas-cli/src/commands/todo.rs:14` | `--all` never fetched or included assignment lists online and always failed offline. | Require complete lists for every active course, add their members, and permit offline execution when all required coverage exists. | `e7a89ea` |
| High | `crates/canvas-core/src/sync/assignments.rs:268` | Assignment and nested submission workflow states could produce duplicate writes and reject normal responses; `pending_review` was treated as unsubmitted. | Keep submission workflow status distinct, correctly recognize submitted work, preserve explicit nulls, and retain grade, attempt, and timestamp metadata. | `9edf811` |
| High | `crates/canvas-core/src/sync/planner.rs:433` | Planner and missing sources did not share field clocks; merging preferred whichever value was already populated rather than the newest supplied value. Sparse status blobs lost omitted fields. | Project planner assignment fields into canonical assignment observations. Preserve individual status fields across sparse/null updates and read the canonical values when merging. | `9edf811`, `c661da2` |
| High | `crates/canvas-core/src/todo/merge.rs:379` | String-valued assignment/parent IDs broke quiz/discussion and checkpoint/peer-review relationships. Pending lookup used planner IDs; cached graded state could hide pending work. | Normalize relationships, retain distinct checkpoint/peer-review keys, resolve pending by assignment or parent, and make submission status unknown before filtering. | `9edf811`, `e7a89ea` |
| High | `crates/canvas-core/src/sync/submission.rs:92` | The submission dataset omitted current attachments, body digest, type, URL, and historical attachments; hashed synthetic storage IDs did not match membership/observation keys. | Store the allowlisted fields M2-b needs, include historical attachments, hash bodies, and use matching normalized numeric keys when Canvas omits a submission ID. | `9edf811` |
| High | `crates/canvas-core/src/sync/planner.rs:483` | Arbitrary planner submission and rubric JSON could be persisted without an allowlist. | Store individual known planner status fields and project rubric/feedback and attachment metadata explicitly; omit attachment capability URLs and raw submission bodies. | `9edf811`, `c661da2` |
| Medium | `crates/canvas-core/src/sync/planner.rs:128` | Requested planner bounds did not match the exclusive coverage end; a wider cache hit was followed by loading the nonexistent narrow membership. | Request exact UTC timestamp bounds, read the returned coverage scope, and trim its items to the requested window. | `9edf811`, `e7a89ea` |
| High | `crates/canvas-core/src/todo/merge.rs:637` | A missing observation clock could make an old `can_submit` appear usable; command reads ignored its age. Buckets lacked excused/eligibility data. | Require a fresh field clock, preserve false eligibility, share complete availability/status with buckets, refresh aged eligibility from the single endpoint, and avoid fetching unreferenced assignments. | `9edf811`, `e7a89ea` |
| Medium | `crates/canvas-cli/src/commands/assignment_read.rs:281` | Required JSON fields were absent or hardcoded; local dates were null, assignment sorting was incomplete, and human views omitted status and day grouping. | Complete shared status/availability payloads, local timestamp siblings, deterministic sorting, day grouping, status colors, Markdown/details, and pending-unknown rendering. Populate registry fixtures and snapshot all four schemas in human and JSON form. | `e7a89ea` |
| Medium | `crates/canvas-cli/src/commands/assignment_read.rs:241` | Incomplete online name resolution was not refreshed; resolution failures returned usage exit 2 and omitted assignment candidates. Malformed target URLs could trigger list resolution. | Refresh needed memberships once, report candidates with exit 6, preserve offline exit 7, and reject malformed URL targets without list requests. | `c661da2`, `e7a89ea` |
| Medium | `crates/canvas-cli/src/session.rs:203` | The new dataset TTL helpers ignored config and dates/counts used UTC instead of the identity zone. | Honor configured assignments/missing/planner TTLs and the saved profile's user timezone, with system-zone fallback. | `e7a89ea` |
| Medium | `crates/canvas-core/src/sync/wire.rs:243` | Omitted nested tool names were interpreted as supplied null; an explicit null submission user ID did not clear its tracked value. | Preserve absent/null distinctions for both metadata paths, with regression coverage. | `bacac97` |
| High | `crates/canvas-cli/tests/review_m1c.rs:1` | Required acceptance tests were missing; the original passing gates did not detect the behavioral failures above. | Added 19 regression tests across core and CLI, including the three-request baseline, every bucket, merge kinds, complete offline `--all`, stale eligibility, pending states and clearing, resolver/error paths, safe cache records, and eight human/JSON snapshots. | `74d086c`, `9edf811`, `c661da2`, `e7a89ea`, `bacac97` |

## Round-3 interface and M2-b handoff

The command enum and dispatch contain `files`, `modules`, `submit`, `submission`, `submission verify`, `submission reconcile`, and all four `receipts` subcommands. Peer-lane stubs return exit 2 with `not implemented`; their parser/stub tests pass. The registry includes all eight requested peer schema constants and parseable placeholder fixtures. Their owning lanes still replace those stubs and fixtures.

The public assignment resolver supports numeric IDs, origin-checked URLs with course agreement, and names over complete memberships. Its class-B path never fetches; class-C/D callers receive the explicit incomplete-dataset signal needed to refresh or report an offline miss.

`SubmissionDataset` and `refresh_submission` expose `submission:assignment:<id>` with all three required includes, complete-object coverage, mutation-epoch checks, status fields, current attachments/body digest/type/URL, comments, feedback, and history attachments. The fixture verifies membership/observation key agreement and absence of capability URLs and raw body content. Journal read helpers remain available for M2-b's `pending_journals`. No journal code or lock/state-transition semantics were changed.

## Needs a decision

None. All identified defects were corrected within the existing specification.
