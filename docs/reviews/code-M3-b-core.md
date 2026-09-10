MERGE
The reviewed defects are fixed on lane/w3; all five required gates pass.
Verified on macOS with local fixtures and subprocesses; native Windows and calendar-client imports were not exercised.

## Scope and authority

Reviewed the package brief, the complete `main..HEAD` worker change (`f49f8cc`), SPEC §§10, 12.3, 12.5, 13–16, and Appendices A/D. Final reviewed code: `a1bcf41`.

The brief predates the SPEC's identity-side download manifests. The current SPEC governs: `.canvas-cli` contains only `dest.json` and `install.lock`; SQLite lives in identity storage. No SPEC, task, store, identity, or other-crate implementation was changed. This report is the explicitly requested exception to the worker brief's docs restriction. Nothing was pushed or merged.

## Gates

All commands used `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/rev-m3b`.

| Gate | Final result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS, Rust 1.97.1 |
| `cargo nextest run --all-features` | PASS: 63 passed, 0 skipped; run `1839c5a3-8c62-4986-8cc0-16a6210a442b` |
| `cargo deny check` | PASS: advisories, bans, licenses, sources; non-fatal duplicate-version warnings |
| `cargo +1.88 check --workspace --all-targets` | PASS, verified compiler is Rust 1.88.0 |

The initial gates also passed (44 tests); their passing result did not detect the defects below. Targeted development runs included all core tests and doc tests. The final nextest run had no leaky tests.

## Defects found and fixed

Locations below refer to the original worker commit `f49f8cc`; paths are relative to `crates/canvas-core/src/`.

| Severity | File:line | What was wrong | What changed | Fix commit |
|---|---|---|---|---|
| High | `download/manifest.rs:289` | Destination-local SQLite and ambient lock reopening bypassed the specified storage boundary; no root registration/fingerprint check rejected copied ownership metadata. | Identity-side manifests, a required destination registry interface and adapter for the migrated state table, root fingerprints, same-root rename updates, copied-root/orphan refusal, and startup recovery. | `caec3b6` |
| High | `download/manifest.rs:329` | Metadata reads followed links; initialization was unlocked; the destination ID was a clock value and was not validated before use. | Root lock precedes metadata initialization; no-follow regular-file checks; UUID-v4 IDs from OS randomness; malformed IDs/JSON refused; identity validation precedes identity-side access. | `caec3b6` |
| High | `download/manifest.rs:395` | Only the destination lock existed, and waiting for the task mutex had no timeout. Reopening the lock by ambient path broke root-rename containment. | Total acquisition deadline includes the task mutex and ordered destination/identity locks. Locks open through retained handles; owned guards survive cancellation of an awaiting caller during critical work. | `caec3b6` |
| High | `download/install.rs:99`, `io/mod.rs:86` | Async transport accepted a synchronous disk writer; cap operations, hashing, sync, and SQLite ran on the async thread. Public channel slots accepted arbitrarily large chunks. | AsyncWrite transport, bounded 64 KiB chunk writer with tested backpressure, blocking filesystem/hash workers, and a bounded process-wide SQLite worker. | `caec3b6` |
| High | `download/install.rs:272` | Transfer errors left part files; byte validation trusted the transport's claimed count; the main install path never attempted the move protocol. | Independent actual-byte counting, early oversized-body rejection, cleanup guards, integrated move/skip classification, reclassification under both locks, retained-parent verification, rename before manifest commit. | `caec3b6` |
| High | `download/install.rs:408`, `download/install.rs:469` | Move/recovery functions depended on callers to lock; moves reopened paths after validation; both-match recovery omitted the unmanaged old file. | Public functions acquire locks and dispatch blocking work themselves; moves retain the old descriptor and parents, persist all phases, and recovery reports both actions. Unsafe recovery paths become per-file results. | `caec3b6` |
| Medium | `download/manifest.rs:174` | A failed explicit SQL transaction could leave the connection inside a transaction; unsigned sizes could wrap; forced replacement could leave two rows claiming one path. | RAII immediate transactions, checked sizes, non-null installed hashes, retirement of superseded path owners, and regression tests for rollback/newer schemas. | `caec3b6` |
| High | `download/contain.rs:62`, `download/contain.rs:97`, `download/contain.rs:124` | Raced opens could expose the wrong error/nonregular descriptor; directory creation races failed unnecessarily; public part/rename helpers accepted multi-component names. | Post-open regular-file validation, no-follow race classification, reparse-point checks, validated single-component helper arguments, concurrent mkdir handling, and Unix directory fsync after installs. | `1f1976d` |
| Medium | `download/sanitize.rs:22`, `download/sanitize.rs:152`, `download/sanitize.rs:183` | Repeated leading dots survived, collision suffixes exceeded the byte budget or missed non-ASCII extensions, and file/directory-prefix collisions made plans un-installable. | Strip leading dots, budget suffixes on UTF-8 boundaries, preserve Unicode extensions, reserve generated names, detect file/parent conflicts, and test collision cascades and long Unicode names. | `36b54c8` |
| Medium | `download/plan.rs:115`, `download/plan.rs:260` | Folder cycles recursed indefinitely; duplicate listing entries planned multiple copies; module titles overrode known file display names; prefixed module components could exceed the budget. | Iterative cycle detection with a typed error, file deduplication, stable module-item order, listing display-name enrichment, final-component budgeting, and preserved empty-module fallbacks. | `36b54c8`, `a1bcf41` |
| High | `ics/mod.rs:104` | UTC timestamps omitted the required `T`, making DTSTAMP/DTSTART/DTEND invalid RFC 5545 values. | Explicit UTC formatting and exact timed-event/deadline assertions. | `40c0e6b`, `d52fc1b` |
| Medium | `ics/mod.rs:53` | Missing civil dates silently became 1970; raw URL/alarm values could inject content lines; UID text was unescaped; longer all-day spans had no warning. | Reject incomplete/invalid inputs, validate alarm grammar and URL controls, escape UID/TEXT values, return span warnings, and test CRLF/multibyte folding and time-zone conversion. | `40c0e6b`, `d52fc1b` |
| Medium | `markdown/mod.rs:19` | Stripping Markdown characters destroyed literal `#`, `_`, `*`, brackets, and some link text; conversion ran synchronously. | Extract decoded plain text from the HTML tree, preserve punctuation, omit script/style content, use blocking conversion, and keep conversion errors content-free. `htmd` remains the chosen converter. | `1034d7f` |
| Medium | `download/install.rs:747` | The claimed two-process test only held a second file descriptor in one process. Important current-SPEC recovery/storage cases were absent. | Real subprocess transfer barrier and competing installs; metadata, registry, lock, rollback, equal-size modification, post-rename crash-state, both-match recovery, wrong-target hash, and backpressure regression coverage. | `caec3b6` |

## Conformance and verification limits

- Action strings include Appendix D's `unresolved_move`. Completed-run mapping covers partial actions, warning-only unmanaged/modified actions, verification mismatch precedence, and dry-run exit zero. Destination-binding/fingerprint refusals expose exit 8; persistence initialization errors expose exit 13.
- Existing Appendix A workspace pins remain unchanged. New direct pins (`getrandom =0.4.3`, `markup5ever_rcdom =0.38.0`) use versions already present transitively in the worker lockfile. `htmd =0.5.5`, `unicode-normalization =0.1.25`, bundled rusqlite, and sha2 remain pinned. Both toolchains and cargo-deny accepted the resulting graph.
- No HTTP implementation, credential access, token persistence, or raw response-body persistence was added. Transfers use fakes; existing redaction tests also pass. Network denial/redaction wiring belongs to the transport/command packages.
- The registry adapter requires the state owner's already-migrated `destinations` table and a verified identity directory/lifetime lock. It deliberately does not create or migrate `state.sqlite`. The in-crate SQLite worker is available for store integration; composition with the other lane's eventual store implementation was not exercised on this scaffold branch.
- Crash tests recreate the post-rename/pre-commit disk/row states; they are not power-loss tests. Directory syncing was exercised on Unix. Native Windows reparse/locking behavior, a real cross-filesystem destination move, and Apple/Google Calendar imports remain outside the host verification performed here. Copy/fingerprint rejection and same-filesystem root rename were tested locally; manual calendar imports belong to M4-b.

## Needs a decision

None requiring a SPEC change. The stale brief's destination-local manifest requirement was resolved against the current SPEC. The state-table migration and shared-executor composition remain the existing cross-lane integration boundary, with an explicit adapter rather than a competing state migration.
