MERGE
All identified M1-a defects are fixed in five review commits on `lane/w2`; no specification change is required.
All five final gates pass, including 49/49 tests and the Rust 1.88 workspace check.

## Scope and evidence

Reviewed worker commit `9013d4cb5675d81b200d2715d87d58dcfa8e8cd6`, the full `main..HEAD` package changes, `tasks/m1a-store-core.md`, and SPEC §§8-10, 12.2 (record, ownership, journal fields), 13-16, and Appendix A. Final code reviewed and tested through `a2509f6`. Changes remain in the owned identity/store modules; this document is the requested review output. Nothing was pushed or merged.

## Gates

Every gate used `CARGO_TARGET_DIR=<checkout>`.

| Gate | Worker baseline | Final result |
|---|---|---|
| `cargo fmt --all --check` | PASS | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS | PASS |
| `cargo nextest run --all-features` | PASS, 33 tests | PASS, 49 tests, zero skipped |
| `cargo deny check` | PASS | PASS: advisories, bans, licenses, sources; permitted duplicate-version warnings remain |
| `cargo +1.88 check --workspace --all-targets` | PASS | PASS |

Also ran `cargo test -p canvas-core`: 29 unit tests passed together in one process, including the shared-worker tests. Final nextest run: `27de28ca-a337-4ee7-bd88-dbd639eea34b`. Tests ran on the native macOS host; this review does not claim Windows/Linux runtime validation.

## Defects found and fixed

Locations below refer to the original worker commit `9013d4c`, under `crates/canvas-core/src/`.

| Severity | File:line | What was wrong | What changed | Commit |
|---|---|---|---|---|
| High | `store/db.rs:56`, `store/mod.rs:39` | Public `Store::open` ignored identity verification and locking. The wrapper could release its lock before the detached database worker finished. | Every store open verifies the document and takes a shared lock; connections retain that lock until they close on the worker. Direct-open and lock-lifetime regressions added. | `04912c8` |
| High | `identity/mod.rs:254`, `store/db.rs:59` | An exclusive lock for one identity could authorize removal of another; caller-supplied database paths could escape the identity layout. | Bind locks to their path, validate all identity-specific paths against the key, and reject symlinked identity/database/sidecar paths before access. Test wrong-lock removal and escaped paths. | `04912c8` |
| High | `identity/mod.rs:331`, `store/db.rs:158` | Predictable truncating temporary-file writes followed symlinks; SQLite state/journal data used ambient creation permissions. | Use unique `create_new` temporary files with mode 0600 at creation, fsync before rename and fsync the Unix directory. Create database files privately and enforce Unix mode 0600. Test that the former temp-path symlink cannot clobber another file and verify file modes. | `04912c8` |
| High | `store/db.rs:63`, `store/db.rs:82` | Every store spawned a SQLite thread, contrary to the one-per-process contract. Bounded-channel sends blocked the current-thread async runtime when full. | Share one process-wide bounded worker, move async queue backpressure onto the blocking pool, and make store disposal nonblocking when the queue is full. Isolate job panics. Test thread identity and runtime responsiveness under saturation. | `04912c8` |
| High | `store/db.rs:169` | Schema version was read before acquiring the migration writer lock; simultaneous first openers could both try migration 1. | Re-read and validate `user_version` inside the immediate transaction before migrating; use transaction rollback on failure. Add simultaneous fresh-identity subprocess coverage. | `04912c8` |
| High | `store/dataset.rs:110`, `store/ops.rs:93` | Epoch checks used bare membership scopes such as `course:1`, while durable mutation scopes include the dataset, such as `assignments:course:1`. The check also happened before entity writes. | Introduce fully qualified epoch scopes and perform the final check under an immediate state transaction held through cache commit. A regression advances the epoch during an upsert and verifies complete rollback. | `29a4032` |
| High | `store/ops.rs:201` | Taking the maximum of independent exact/prefix counters hid increments to a smaller matching counter. | Sum matching counters with overflow detection, so any exact or prefix increment invalidates the prior observation. Add overlapping-prefix coverage. | `29a4032` |
| Medium | `store/ops.rs:88` | Window lookup required an exact scope key, missing wider valid coverage; mismatching contexts/coverage could be returned as stale usable rows. | Search dataset coverage for context-matching containing windows, prefer a fresh eligible row, and return Miss for unrelated coverage. Validate both requested and stored scopes and reject future-dated freshness. | `29a4032` |
| High | `store/dataset.rs:121` | Ingestion committed partial pages even when options declared failure/incompleteness. The original interruption test manually changed SQL metadata instead of exercising ingestion. | Failed refreshes preserve entities, memberships, observations, and previous coverage; only stale/error metadata changes. Actual ingest failures roll back first. Add partial-page and malformed-second-page regressions through the public API. | `5a68f7f` |
| Medium | `store/dataset.rs:138` | Repeated entities across pages violated membership uniqueness and aborted otherwise usable refreshes. | Deduplicate exact-scope membership and count while still applying per-field observations from every page. Verify other scopes and unreferenced entities survive. | `5a68f7f` |
| High | `store/dataset.rs:243` | Lexicographic timestamp comparison incorrectly ordered whole-second and fractional-second observations, rejecting newer values or allowing older values to overwrite them. | Compare parsed timestamps. Test fractional-second explicit null followed by an older response. | `5a68f7f` |
| Medium | `store/dataset.rs:28`, `store/dataset.rs:525` | Three-state fields lacked deserialization. Grade letters/course IDs were observed without being stored; unsupported/duplicate fields and non-normalized fake IDs could corrupt observation bookkeeping. | Add absent/null/value deserialization, persist the remaining supported grade columns, and validate fields and keys before recording observations. Test independent period clocks and explicit-null grade values. | `5a68f7f` |
| Medium | `store/migrate.rs:323`, `store/migrate.rs:347` | Failure transitions shared only `terminal_at`, losing distinct transition times on later reconciliation. The requested `identity` metadata table was instead named `identity_meta` and was never populated. | Add distinct failure-transition timestamps, use `identity`, and populate/verify identity metadata on open. Add checks for specified journal/source/evidence/mode values and schema coverage. Migration remains the package-owned `0001_initial`. | `f409d45` (metadata initialization began in `04912c8`) |
| Medium | `store/ops.rs:271` | Pending-journal supersession compared timestamp strings and used a separate query, mishandling later attempts within the same second. | Evaluate one fetched journal snapshot using parsed timestamps. Test planned/active states, acknowledgment, subsecond supersession, terminal failures, and the one-active-journal index. | `f409d45` |
| Medium | `store/tests.rs:289` | The concurrent-reader clear test discarded read errors and did not establish an active reader snapshot. | Hold a verified read transaction across clear/VACUUM, assert the old snapshot remains readable, then assert a new snapshot sees zero rows. | `a2509f6` |

## Conformance and boundaries

The complete cache table set, scoped grade keys, per-field observations, fetch-log columns, credential cleanup flags, journal fields, admission index, and identity metadata were checked. Cache clear retains the database files and durable epochs. Removal still performs credentials → directory → profiles, preserves the directory/profiles after credential failure, retains the lock file, and re-verifies waiting openers. The full SPEC pending definition includes `planned`, acknowledgment, and supersession; it governs the package brief's abbreviated state list.

Dependency versions/features still match Appendix A where specified, including bundled rusqlite, fs4, Tokio, serde, jiff, and sha2. Existing exact uuid/tempfile pins were retained; no review dependency changes were needed. The host MSRV gate passed.

The package introduces no network code or token storage. Built-in entity ingestion writes selected fields; ingestion-error persistence uses a fixed safe summary rather than formatting arbitrary underlying errors. Explicit fetch-error summaries are documented as requiring sanitized input. Actual API-response allowlisting and CLI exit-code rendering remain later-package integration work; this review verifies library persistence errors, identity-change rejection, and newer-schema rejection rather than claiming live CLI behavior for those stubs.

## Needs a decision

None.
