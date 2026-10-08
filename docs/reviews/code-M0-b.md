MERGE
M0-b defects found in this review are fixed and committed on `lane/w1`; no decision remains.
All five final gates pass, including 65 tests and the Rust 1.88 workspace check.

## Scope and verdict

Reviewed worker commits `691a041` and `2348b18`, `git log main..HEAD --stat`, and the full package diff against the current `docs/SPEC.md` §§7, 10, 11, 14-16 and Appendices A/B, with Appendix D checked for model coverage. Final code revision: `815e6573d4c6c391588c55fd8de7516cd520bbf9`.

The current SPEC governs where the older package brief differs: lower governor observations always apply without reducing the watermark; cooldown targets 350 and exits on an applied sample at least 300; tracked ingestion fields use `Supplied`; valid upload redirects are returned as completion handoffs rather than automatically followed or discarded. No SPEC change was needed.

This verdict covers the M0-b contracts. Streaming upload/download bodies, multipart ordering, size validation, finalization/journaling, filesystem containment and allowlisted persistent response records belong to later packages. Transfer URL/header/handoff rules have unit coverage here; they are not represented as end-to-end HTTPS streaming tests. API behavior is exercised with local wiremock servers, without a live Canvas account.

## Gate results

Every Cargo invocation used `CARGO_TARGET_DIR=<checkout>`.

| Gate | Final result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS; Rust 1.97.1 |
| `cargo nextest run --all-features` | PASS; 65 passed, 0 skipped, 11 binaries; 48 API tests and 17 CLI tests. Final run: `a37eee99-ac38-4e69-8260-1cef993da159` |
| `cargo deny check` | PASS; advisories, bans, licenses and sources all OK. Existing duplicate-version warnings remain nonfatal under repository policy. |
| `cargo +1.88 check --workspace --all-targets` | PASS; Cargo/Rust 1.88 toolchain |
| `git diff --check` | PASS |

The original 40-test suite passed before fixes; its coverage did not detect the defects below. Regression coverage now includes real queued governor admissions, the 3-low / 1-lower / 2-high observation order, first-low cooldown, refill bootstrap and cost-one recovery, nonrepeating silence reset, exact paused-time 1/2/4/8-second HTTP retry boundaries for both 429 and throttle-403 exhaustion, retry readmission, wrapped pagination, redirect accounting, nested model normalization and tracing span/event redaction.

## Defects found and fixed

Locations refer to the final source revision. Severity: high = credential exposure or materially incorrect request/data behavior; medium = missing contract behavior or verification.

| Severity | File:line | What was wrong | What changed | Commit |
|---|---|---|---|---|
| High | `crates/canvas-api/src/redact.rs:25` | The byte-indexed redactor could panic on Unicode before a secret, leak later nested `upload_params` values, and miss encoded query keys. | Detect decoded keys, preserve ordinary Unicode text and suppress complex secret-bearing blobs as a whole. Added nested, encoded and Unicode regressions. | `13973c8` |
| High | `crates/canvas-api/src/redact.rs:93`, `crates/canvas-api/src/redact.rs:154` | The `Layer` implementation was a no-op; field formatting inspected values but not secret field names, exposing `token`, `Authorization`, numeric secrets and span values. | Replaced the misleading no-op with a real redacting formatting-layer constructor; redact by field name and value, including span creation and updates. | `13973c8` |
| High | `crates/canvas-api/src/error.rs:15`, `crates/canvas-api/src/request.rs:421` | Derived request/transfer diagnostics exposed raw bodies, headers and signed URLs; forbidden error formatting could print raw response text. | Limit request/response Debug to safe metadata, omit forbidden response bodies from Display, and retain redacted validation diagnostics. | `13973c8` |
| High | `crates/canvas-api/src/governor.rs:203` | A cooldown probe could overlap previously admitted requests. Multiple queued callers could reuse timers that had already elapsed and issue probes back-to-back. | Serialize admission/probe timing, wait for existing requests to drain, and retain separate lane semaphores. Added queued API/storage probe tests. | `e8a7b99` |
| High | `crates/canvas-api/src/governor.rs:352` | Header-silence reset left its clock expired, allowing every subsequent idle admission to reset to full; stale refill evidence also survived. | Start a new silence epoch and clear refill/sample state with the watermark. Check reset both with and without active requests. | `e8a7b99` |
| Medium | `crates/canvas-api/src/governor.rs:381`, `crates/canvas-api/src/governor.rs:457` | Refill evidence could be inferred from unknown or still-overlapping requests; nonfinite/negative headers poisoned state; jitter sampled the elapsed time immediately after creating an Instant. | Retain request evidence until release, require genuine non-overlap, reject invalid numeric observations and use a varying clock sample for bounded jitter. Add watermark/bootstrap/workload coverage. | `e8a7b99` |
| High | `crates/canvas-api/src/models/assignment.rs:34`, `crates/canvas-api/src/models/submission.rs:74`, `crates/canvas-api/src/models/course.rs:18` | Only `can_submit` preserved suppliedness. Core/detail/status and scoped grade fields collapsed absent and explicit null, preventing correct field-observation updates. | Use explicit `Supplied` deserializers for the tracked fields and add three-state fixtures covering assignment, submission and scoped grade values. | `ffed891` |
| Medium | `crates/canvas-api/src/serde_util.rs:255`, `crates/canvas-api/src/models/assignment_group.rs:30` | Nested IDs/ID vectors rejected strings; several module/enrollment/planner URLs and timestamps remained raw strings; embedded group assignments bypassed typed normalization. | Add ID-vector and supplied-URL helpers, normalize nested model values and use typed embedded assignments. Preserve absent/null equivalence for module items. | `ffed891` |
| Medium | `crates/canvas-api/src/serde_util.rs:17` | Panic unwinding could leave another origin installed in the thread-local deserialization context; URL decode errors could echo signed references. | Restore the origin with an RAII guard and remove raw URLs from decoder error text. | `ffed891` |
| Medium | `crates/canvas-api/src/models/course.rs:52`, `crates/canvas-api/src/models/planner.rs:12` | Course teachers were discarded; planner assignment/parent keys and scheduling fields were unavailable through normalized models. Missing-submission includes were also dropped. | Retain teachers, course/planner includes, typed planner IDs/dates and unknown planner payload fields. | `ffed891`, `1f83389` |
| High | `crates/canvas-api/src/models/grading_period.rs:32` | A malformed wrapped response with no collection silently became a successful empty page, allowing downstream membership replacement with no data. | Require the collection property while still accepting an explicitly empty array; test missing/null wrappers. | `1f83389` |
| High | `crates/canvas-api/src/request.rs:156` | All redirect hops shared one issue/permit, undercounting requests and suppressing newer observations. A redirected GET also retained POST content headers. | Give each hop a fresh admission, issue and route cost; drop body/content headers on conversion to GET, enforce the hop limit and reject unsupported redirect statuses. | `a575b93` |
| High | `crates/canvas-api/src/request.rs:134`, `crates/canvas-api/src/request.rs:385` | Exhausted throttle-403 retries returned Forbidden instead of RateLimited. HTTP-date Retry-After was ignored; broad timing assertions did not verify the retry schedule. | Return RateLimited after the initial request plus four retries, parse both Retry-After forms and test each exact HTTP retry boundary plus cooldown readmission. | `a575b93` |
| High | `crates/canvas-api/src/request.rs:254`, `crates/canvas-api/src/request.rs:309`, `crates/canvas-api/src/request.rs:328` | Supplied Authorization headers bypassed off-origin stripping; upload POSTs received a token and could be replayed; valid completion redirects were discarded. Download status errors were returned as raw success values. | Recompute credentials for each download hop, strip supplied credential headers, send no upload bearer, never replay upload POSTs, return validated handoffs, and classify exhausted throttles/Canvas denials/storage expiry. | `a575b93` |
| Medium | `crates/canvas-api/src/lib.rs:120`, `crates/canvas-api/src/lib.rs:308` | Transfers inherited the API total timeout and decompression. The public API builder had no public execution entry point, and DELETE 204 failed JSON decoding. | Use a separate transfer HTTP client without total timeout/decompression, validate client origin/token header construction, expose `send_api` and decode no-content success as null/unit. | `a575b93` |
| High | `crates/canvas-api/src/lib.rs:469` | Pagination inspected only one Link header, split inside quoted/URL commas, did not handle relation lists or relative links and could silently omit later pages. Wrapped pagination had no HTTP integration test. | Parse all header values and quoted parameters, resolve next links against the final response URL, retain same-origin checks, and exercise both pages of the wrapped fixtures. | `815e657` |
| Medium | `deny.toml:21` | The inherited workspace license allowlist permitted MPL-2.0 despite §15's copyleft prohibition. | Remove the unused allowance. The locked dependency metadata contains no MPL package, and the stricter gate passes. | `e319c7a` |

## Dependency and compatibility notes

Appendix A's existing exact dependency pins remain unchanged. `canvas-api` now directly declares `httpdate = "=1.0.3"` for HTTP-date Retry-After parsing; this version was already in Cargo.lock transitively. No workspace dependency was added, and the lock change adds only that direct dependency edge.

The required `Option` to `Supplied` ingestion-contract corrections mean downstream code must handle absent/null/value explicitly. They do not change CLI JSON schemas or numeric exit-code definitions. Throttle exhaustion now yields the specified `Error::RateLimited`, allowing callers to select exit 5. Full CLI error rendering remains outside this API package.

No push or merge was performed. Other crates and existing task files were not edited; the review includes the one-line workspace license-gate correction, API changes, their dependency edge, and this report.

## Needs a decision

None. All defects identified within this review were fixed without changing the SPEC. The remaining upload/download body and persistence work is the package brief's explicit later-package boundary, not a deferred M0-b defect.
