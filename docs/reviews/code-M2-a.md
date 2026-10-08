MERGE
The M2-a defects found in this review are fixed and committed on `lane/w3`.
All five gates pass; 169 tests pass; no spec decisions remain.

## Scope and baseline

Reviewed `tasks/m2a-transport-journal.md` against SPEC §§9-12.2, 15-16 and Appendices A/D, including the existing request/governor, store, and I/O APIs. The review task was present in the main checkout and explicitly named this worktree. Worker tip: `9de2d6f`. The required initial merge brought in main at `c2caeff` as `759012c`; conflicts in the API client/request executor were resolved by retaining main's reviewed transfer safeguards. Subsequent main commits through `5ce0f6e` contain task briefs only.

Reviewed code and final gates cover `d72b27d`. No push or merge into main was performed. No schema migration was needed.

## Gates

All commands used `CARGO_TARGET_DIR=<checkout>`.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo nextest run --all-features` | PASS :  169 tests, 0 failures, 0 skipped; run `d1d1c448-9833-4132-b4f2-a740a56df98a` |
| `cargo deny check` | PASS :  advisories, bans, licenses, sources; existing duplicate-dependency warnings remain non-fatal |
| `cargo +1.88 check --workspace --all-targets` | PASS :  Cargo/Rust 1.88 toolchain |

Normal toolchain: Rust 1.97.1. Appendix A version pins were preserved. TLS fixtures add only a dev dependency on the already locked `tokio-rustls =0.26.5`; production HTTP remains on the pinned reqwest/rustls stack.

The initial post-merge test run failed because upload fixtures used HTTP. A later complete run exposed an inherited concurrent WAL-initialization failure. Both were fixed before the final passing run. The WAL regression additionally passed **30/30** consecutive two-process runs after a pre-fix reproduction identified journal-mode initialization as the failing statement.

## Defects found and fixed

Locations identify the corrected code unless explicitly marked as the worker version.

| Severity | File:line | What was wrong | What changed | Commit |
|---|---|---|---|---|
| High | `crates/canvas-api/src/upload.rs:67` | Upload read the entire file into memory and built another complete multipart buffer. It had no write-progress deadline; reusing the download read timeout also imposed the wrong timeout while waiting for upload response headers. | Bounded 64 KiB producer chunks, one queued file chunk, incremental SHA-256, cancellation-coupled producer/request futures, and a separate upload client with connect timeout plus a 60-second progress watchdog and no total timeout. | `753fa67` |
| High | `crates/canvas-api/src/upload.rs:48` | `serde_json::Map` sorted upload parameters, violating wire order. Handwritten disposition headers did not safely encode supplied names. | Direct ordered-map deserialization and reqwest multipart encoding; parameters remain in received order and `file` is last. A raw JSON fixture asserts all parameter positions. | `753fa67` |
| High | `crates/canvas-api/src/upload.rs:189` | Completion handling could lose the actual status on a missing Location and return Decode for a missing final ID. The worker's transfer changes weakened main's HTTPS/credential protections. | Preserve reviewed request rules; accept only valid 201 IDs or supported same-origin GET handoffs, reject other outcomes as UploadIncomplete with the actual status, and never replay the multipart POST. | `759012c`, `753fa67` |
| High | `crates/canvas-api/src/download.rs:52` | The streaming download path accepted HTTP/userinfo, reused one governor admission across redirect hops, ignored throttle-shaped 403 bodies and Retry-After, and reported short Content-Length bodies as Network. | Shared HTTPS/userinfo checks on every hop, per-hop admissions and observations, throttle-first classification with bounded retries, Retry-After handling, and SizeMismatch for truncated declared bodies. | `753fa67` |
| Medium | `crates/canvas-api/src/download.rs:87` | Reading every error body before classification could turn a final Canvas 401/404 into Network when the error body failed. | Only 403 requires body inspection for throttling; final denial status survives an unreadable body. | `648072d` |
| High | `crates/canvas-core/src/journal/ops.rs:145` | Creation relied on an unenforced admission-lock convention, and state writers accepted journal IDs without proof of ownership. A lock acquired under another identity directory could otherwise authorize the wrong store. | Creation requires the matching admission guard. Writers require a matching held owner guard, and mutation/recovery paths verify that the lock directory belongs to the opened state database. Unique-index conflicts alone map to InProgress. | `9b10cdb` |
| High | `crates/canvas-core/src/journal/ops.rs:233` | Journal writes used deferred transactions, arbitrary state edges were allowed, most transitions omitted epoch invalidation, and the success path used an incorrect assignment-group prefix. | Immediate transactions, legal state edges and expected-state checks, atomic epoch bumps for transitions, mandatory posting timestamp, and `assignment_groups:course:<id>:*`. Confirmed outcomes use dedicated receipt transactions. | `9b10cdb` |
| High | `crates/canvas-core/src/journal/ops.rs:327` | Uploaded IDs could be appended outside uploading, corrupt arrays were silently reset, and IDs were not associated with individual frozen files. | Guard and validate appends, reject malformed/duplicate IDs, and atomically update the indexed intended file with its Canvas ID. Receipts preserve file association when concurrent uploads finish out of order. | `9b10cdb` |
| High | `crates/canvas-core/src/journal/ops.rs:386` | The stored receipt was only a small subset of the required document; matched outcomes and receipt/readback consistency lacked protected helpers. Success status was not stored. | Build the durable receipt from identity + frozen intent + allowlisted evidence inside the success transaction. Add guarded matched/readback helpers, enforce evidence/attribution and attempt/file checks, store observed HTTP status, and update receipt and journal readback together. | `9b10cdb` |
| Medium | `crates/canvas-core/src/journal/ops.rs:599` | Journal reads omitted required recovery fields; acknowledgment returned inconsistent conflict errors; supersession lacked a public helper. | Expose all stored journal fields, normalize guard failures, add read-only supersession and explicit assumption helpers, enforce the full 30-minute threshold and current-history absence input, and support clearing obsolete server matches. | `9b10cdb` |
| High | `crates/canvas-core/src/journal/locks.rs:53` | Lock files could follow symlinks, journal IDs could escape the lock directory, creation did not set private permissions, and probe errors were reported as owner absence. | Capability/no-follow directory and file opens, UUID validation, private creation permissions, regular-file checks, persistent lock files, and propagated probe errors. | `9b10cdb` |
| High | `crates/canvas-core/src/journal/record.rs:115` | Generic response/receipt JSON patches could bypass the allowlist; malformed evidence was accepted; legitimate submitted URLs containing signing-like query names were discarded; local timestamp fields were missing. | Remove unsafe generic receipt/response writes, canonicalize candidate records, validate frozen intent and evidence, retain the explicitly submitted URL while dropping attachment URLs, and render companion timestamps in the frozen identity zone or system fallback. Raw response/body content is hashed in memory. | `9b10cdb` |
| High | `crates/canvas-core/src/journal/crash_tests.rs:170` | The worker kill test killed a process holding an unrelated lock, then recovered a separately created journal. Required publication-boundary, multiprocess probe/race, and transaction-failure coverage was absent. | Kill the actual journal owner at 14 boundaries; inspect uncommitted-row rollback; run independent live-owner probes and competing recoverers; race two creating processes; test receipt/epoch rollback, readback, matching, pending retirement, symlink refusal, and file-ID association. | `9b10cdb` |
| High | `crates/canvas-core/src/store/db.rs:269` | The full gate exposed a main-inherited race: concurrent fresh openers could fail at `PRAGMA journal_mode=WAL` even with a busy timeout configured. | Retry only BUSY/LOCKED journal-mode initialization within the existing five-second contention budget; verify WAL was enabled and restore the normal busy timeout. | `d72b27d` |

## Verification and integration notes

Transport verification uses wiremock behind a local TLS front-end with an explicit fixture trust root. Production HTTPS checks remain active. Tests cover credential attachment/stripping, identity encoding, every supported upload handoff, empty/non-JSON completion, missing/off-origin Location, no POST replay, missing/truncated Content-Length, throttle ordering, and stream backpressure/idle behavior. The producer also runs beyond 60 seconds under continued progress in a paused-time test.

Journal verification includes real helper subprocesses, durable SQLite state after forced termination, simultaneous creation, two recoverers, and live-owner probes during every active phase. The success transaction is tested both by process termination and an injected epoch-write failure. Receipt assertions verify identity, intent, per-file IDs, attribution, status, readback consistency, local timestamps, and absence of raw body/signed attachment URL content.

For M2-b: retain the journal-holder guard through network work and the receipt-export attempt; release the admission guard after publication. Pass the frozen file index when journaling an upload ID. `IntendedPayload` defines the accepted intent shape and optional identity `time_zone`. Use the typed success/matched/readback helpers rather than generic state patches for confirmed outcomes. Synchronous journal operations belong on the blocking executor. Network history selection, actual submission orchestration, CLI rendering, and writing receipt export files remain M2-b work as the brief specifies; no live Canvas submissions were made by this review.

No store/I/O accessor or migration was added. The narrowly scoped shared-store WAL fix was required by the failing workspace gate.

## Needs a decision

None. The fixes follow the existing specification.
