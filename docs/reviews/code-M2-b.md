MERGE
M2-b reviewed against SPEC v0.8; the defects below are fixed and committed on `lane/w3`.
All five gates pass; the explicitly permitted M1-c integration remains deferred.

## Scope and gate results

Reviewed `tasks/m2b-submit-receipts.md`, its cited SPEC sections and appendices, the worker commits, and their implementation diff. The requested initial merge is `791d754` (main parent `e012d30`). Final code/test revision: `5179fe3`. No push or subsequent merge was performed. Main advanced independently to `1669e8e` with M3-a during this review; these results describe this worktree, not a hypothetical merge with that newer main.

Every cargo gate used `CARGO_TARGET_DIR=<checkout>`.

| Gate | Final result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo nextest run --all-features` | PASS: 307 passed, 0 skipped, 19 binaries; snapshot updates disabled (`INSTA_UPDATE=no`). Run `81f98340-9d1e-49f8-b80b-546fb2a37ae2`. |
| `cargo deny check` | PASS: advisories, bans, licenses, sources all OK. Existing duplicate-version warnings remain non-fatal. |
| `cargo +1.88 check --workspace --all-targets` | PASS |

`git diff --check` also passed. Appendix A workspace dependency pins were retained. HTTPS integration tests reuse the existing pinned TLS fixture through an optional `test-support` feature; normal default builds do not enable that feature. The initial gates also passed (284 tests), despite the behavioral defects found below.

## Defects found and fixed

Locations refer to the corrected source. All paths are relative to the repository root.

| Severity | File:line | What was wrong | What changed | Commit |
|---|---|---|---|---|
| High | `crates/canvas-core/src/submit/reconcile.rs:87` | Recovery acquired the journal-holder lock and then called a helper that tried to acquire it again. Dead-owner journals could remain active, and lock contention could become a persistence error. | Recover and re-read under one owner lock; return structured live-owner recovery, correct ineligible-state refusal, and idempotent confirmed outcomes without blocking behind the exporter. | `8b2fdc4`, `e43d509` |
| High | `crates/canvas-core/src/submit/reconcile.rs:216` | Missing current-attempt data was treated as the baseline, allowing an unsupported negative assumption. Previously recorded text matches survived later reads that no longer matched. | Require an explicitly observed baseline before permitting an assumption; evaluate visibility before time/content filters; replace or clear server-match evidence on each read. | `8b2fdc4`, `e43d509` |
| Medium | `crates/canvas-core/src/submit/reconcile.rs:332` | File candidates included unrelated attachment sets; text/URL candidates could contain attachment IDs. Returned timestamps and diagnostics lost evidence available in the journal. | File candidates require exact ID-set equality; text/URL candidates have empty attachment arrays; preserve local timestamps, response metadata, attribution, and actionable unresolved-outcome messages. | `8b2fdc4`, `e43d509` |
| High | `crates/canvas-core/src/submit/verify.rs:142` | Verification preferred potentially stale or corrupted export files, accepted a receipt without its journal, and checked only the identity key. Missing file IDs could disappear through filtering. | Load from authoritative state; validate identity, receipt/journal binding, course, assignment, kind, attempt, intended files and uploaded/posted ID sets before network access. Export filenames are no longer treated as arbitrary readable paths. | `a957973`, `73c21bf` |
| High | `crates/canvas-core/src/submit/verify.rs:362` | Opening `tmp` through an ambient path followed symlinks. Predictable files were reused without truncation, downloads were buffered in memory, and local containment failures escaped the per-file unavailable result. | Traverse from the retained identity capability with no-follow checks; use fresh mode-0600 part files; stream, hash on a worker, and remove them; preserve mismatch precedence and refresh an expired storage URL once. | `a957973` |
| High | `crates/canvas-api/src/submission.rs:117` | The fallback diagnostic copied the first 200 characters of an arbitrary response body into durable journal error text. | Retain allowlisted, redacted error fields; use a constant diagnostic for unrecognized bodies. | `a957973` |
| Medium | `crates/canvas-api/src/submission.rs:78` | Exhausted throttles and refused redirects lost the received HTTP status/body classification, preventing the required immediate positive-evidence check. | Submission requests retain those final responses while keeping shared retry and redirect-containment rules. | `6bc40c4` |
| Medium | `crates/canvas-core/src/submit/preflight.rs:117` | CLI input reads preceded fresh eligibility/admission checks; input allocations were unbounded; disallowed extensions were not checked. Attempt addition could overflow. | Freeze on a worker after fresh checks under admission; bound text/HTML reads, stream file hashing, check extensions and widen attempt arithmetic. | `b497062`, `73c21bf` |
| Medium | `crates/canvas-core/src/submit/freeze.rs:129` | Text transformation trimmed content and converted lone CR characters beyond the specified CRLF normalization. URL validation accepted malformed hosts. | Preserve whitespace and lone CR, normalize CRLF only, retain the required HTML escaping, and validate URLs using the URL parser. | `b497062` |
| High | `crates/canvas-core/src/submit/execute.rs:262` | Upload execution reread whole files into memory, swallowed failed journal transitions, and could release ownership while cancelled upload tasks were still unwinding. POST-body construction happened after marking `posting`. | Stream file descriptors with concurrency two; propagate durable transition failures; cancel and drain siblings before return; build the request before marking it dispatched. | `e43d509` |
| Medium | `crates/canvas-core/src/journal/ops.rs:576` | Stored readback contained only the public projection, and enrichment did not update the stored receipt's text reference digest. Missing posted-attempt readback could be silent. | Keep the full allowlist in the journal and the defined public projection in the receipt, update the reference digest transactionally, bind enrichment to the selected attempt, and report missing enrichment/export warnings. | `e43d509` |
| High | `crates/canvas-cli/src/commands/submission.rs:117` | Verify/reconcile skipped network-token identity validation; network errors were flattened to exit 13. Reconcile aborts omitted durable journal context. | Validate the network identity when needed, after receipt-local validation; retain API error classifications and request counts; include journal state and posted identity on reconciliation aborts. | `73c21bf` |
| Medium | `crates/canvas-cli/src/commands/submit.rs:26` | Successful submit output omitted `posted` and uploaded file IDs; failure output lost known files/state and request counts. `--yes` hid the plan and due warning. Ctrl-C did not implement exit 11. | Hydrate output from durable state, map terminal recovery/refusal correctly, retain abort details and telemetry, render the plan and due warning, and support terminal cancellation with exit 11. Preserve known identity time zones in receipt timestamps. | `73c21bf` |
| Medium | `crates/canvas-cli/src/commands/receipts.rs:67` | Class-B receipt commands constructed a network client; course filtering accepted only numeric IDs; human output concealed owner, acknowledgement and server-match information; raw-output write errors were ignored. | Use local sessions and the existing class-B course resolver; show journal/evidence status and report output failures. | `73c21bf` |
| Low | `crates/canvas-core/src/receipts/ops.rs:584` | Bare relative export names could fail parent-directory synchronization; chmod reopened the installed name unnecessarily; tie ordering was reversed; receipt tests imported Unix APIs unconditionally. | Normalize an empty parent to `.`, rely on the private temporary file's mode, sort ties by journal ID ascending, and guard Unix-only assertions/imports. | `026ca38` |
| High | `crates/canvas-core/src/submit/review_tests.rs:102`; `crates/canvas-cli/tests/review_m2b.rs:133` | Required adversarial orchestration and actual CLI output/exit tests were largely absent; fixture snapshots alone did not exercise the wired commands. | Add HTTP/HTTPS lifecycle cases, identity/containment verification, ordered preflight and upload tests, actual CLI JSON/human snapshots, error/exit snapshots, and real terminal cancellation. Extend subprocess crash coverage to call receipts, reconcile and the pending hook while owners are live, and rebuild exports after a success-transaction crash. | `8b2fdc4`, `a957973`, `5179fe3` |

## Verification evidence

- Reconciliation covers commit-then-400/500, branded 504 followed by an empty history and later matching commit, timeout and malformed 2xx, exact file matches with 0/1/2 entries, unproven file attribution, text server matches, stale evidence clearing, visibility outside the time window, and the 30-minute assumption rule.
- Verification covers file match/mismatch/missing/extra/unavailable, expired storage URLs, symlinked `tmp`, invalid receipt bindings with zero requests, URL refusal, stale exports, text with/without a server digest, and selecting the posted attempt instead of the latest one.
- Execution covers bounded concurrent uploads, changed/missing files and failed uploads without a submission POST, fresh eligibility before input access, long Unicode comments, failed readback followed by idempotent enrichment, supersession and acknowledgement clearing pending status.
- Existing process-kill/admission tests now exercise the actual receipt/reconcile APIs and pending hook in a second process, and rebuild the receipt export after the success transaction survives a killed owner. No lock file is deleted.
- CLI snapshots exercise submit, receipts list/show/export, receipt documents, verify and reconcile, plus exits 2, 4, 8, 9, 10, 11, 12 and 13. A PTY test checks both answering no and Ctrl-C. Test identities and controlled localhost HTTP/HTTPS servers are used; no live Canvas account submission was performed. The Rust 1.88 check is for the host target, not a cross-platform runtime test.

## Deferred to M1-c integration

- Wire submit's full course/assignment/URL resolution against M1-c, including origin/course agreement and dataset-backed assignment matching. The current CLI still requires numeric course and assignment operands.
- Replace the `submission <course> <assignment> [--history]` stub with the M1-c submission dataset, freshness/offline behavior, history, public schema projection and `pending_journals`; replace/validate its placeholder fixture against the real command.
- Run the complete `todo` and `submission` command reader/renderer paths during active submission phases once M1-c lands. The current subprocess tests exercise the shared pending hook and actual receipts/reconcile APIs; they do not claim an implemented `todo` renderer.
- Repeat integration acceptance and all gates after that integration. These permitted dependencies are not counted as M2-b defects.

## Needs a decision

None. No specification change was required for the fixes above.
