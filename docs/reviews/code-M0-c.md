MERGE-AFTER-DECISION
The implementation defects found below are fixed and committed on `lane/w1`; two conflicting spec requirements remain for Rolf.
All five requested native gates pass, including 176 tests; macOS keyring set/get/delete passed separately, while other release targets remain unverified.

## Scope and gates

Reviewed `tasks/m0c-config-auth.md`, its cited SPEC sections, §15, the worker history and diff against `main`, and the existing API/core interfaces. The required initial `git merge main` completed as `28e5d4f`. Reviewed code after fixes: `2b1acb0`. No push or subsequent merge was performed.

Every Cargo invocation used `CARGO_TARGET_DIR=<checkout>`.

| Gate | Final result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo nextest run --all-features` | PASS: 176 passed, 0 skipped |
| `cargo deny check` | PASS: advisories, bans, licenses, sources; duplicate-version warnings remain nonfatal |
| `cargo +1.88 check --workspace --all-targets` | PASS on `aarch64-apple-darwin` |
| `CANVAS_TEST_KEYRING=1 cargo test -p canvas-cli --bin canvas credentials::review_tests::macos_keyring_round_trip_opt_in -- --exact` | PASS: real native keyring set, get, delete, and subsequent `NoEntry`; unique test account removed |

The normal test run's keyring test returns early unless opted in; the separate command above actually exercised the backend. File-store/failure tests use isolated temporary roots and debug-only injection. Wrong-owner rejection has a unit test of the ownership comparison; no privileged ownership changes were performed. The existing strict core waiting-opener removal test passed in the full suite.

The first full regression run exposed a concurrent-login failure; the corrected WAL initialization and both concurrent-login regressions pass in the final suite. A new transport test initially used a pooled Wiremock server as though dropping its handle closed the listener; it now uses a deterministic TCP reset.

`--all-targets` means Cargo targets for this host, not every release platform. Neither musl architecture, Intel macOS, nor Windows MSVC was built or executed here. Their release-target evidence required by the brief is still outstanding. Unix-only fallback tests/imports are now gated so they do not assert file-fallback behavior on Windows.

## Defects found and fixed

Locations refer to the resulting source; paths beginning `src/` or `tests/` are relative to `crates/canvas-cli/`.

| Severity | File:line | What was wrong | What changed | Commit |
|---|---|---|---|---|
| High | `src/commands/auth.rs:407`, `src/credentials.rs:224` | The supposedly hidden prompt echoed input; backend failure silently stored plaintext. | Use a hidden terminal reader; require explicit interactive consent for real file fallback; refuse noninteractive fallback. Added the token-settings browser offer. | `3ded806`, `6726a4f` |
| High | `src/credentials.rs:83` | Keyring `Display` can internally Debug attached data; byte-index truncation could panic on Unicode; locked-access classification was incomplete. | Emit fixed sanitized application messages, inspect platform text only for classification, and test secret-bearing variants and Unicode. | `3ded806` |
| High | `src/credentials.rs:695`, `src/token.rs:18` | TOML parse errors could include credential source text; token-bearing structs derived raw Debug output. | Suppress credential parse content, remove file-model Debug, and wrap resolved tokens in the existing `canvas_api::Secret`. | `3ded806`, `2b1acb0` |
| High | `src/credentials.rs:742`, `src/credentials.rs:755` | A FIFO could block before fstat; hand-coded no-follow flags and a writable ownership-probe file weakened reads. | Use rustix no-follow/nonblocking open and effective UID, enforce regular-file/0600 ownership checks, and use unique create-new replacement files. Added unsafe-file regressions. | `3ded806` |
| High | `src/credentials.rs:569` | Identity removal ignored backend deletion failures and could delete data/profiles while credentials remained. | Preserve every non-NotFound deletion failure, attempt both stores, and abort before directory/profile removal. | `3ded806` |
| High | `src/credentials.rs:593`, `src/credentials.rs:429` | Status missed a crash-induced hash mismatch in the active store; logout discarded inaccessible inactive-store evidence. | Report mismatching active entries as stray; preserve deletion work for unreadable stores and both durable failure flags. | `3ded806`, `2b1acb0` |
| High | `src/origin.rs:11` | Arbitrary HTTP origins could receive tokens. | Require HTTPS; permit only literal loopback HTTP under a debug-only test gate. | `6726a4f` |
| High | `crates/canvas-core/src/identity/mod.rs:133`, `src/selection.rs:476` | Dot operands passed the key parser; login could create/overwrite identity metadata without coordinating creation/removal. | Reject dot components; initialize once under the shared identity lock and credential lock, recheck paths, preserve generations, and retain the lock through store opening. | `6726a4f` |
| High | `src/selection.rs:126`, `src/selection.rs:149`, `src/selection.rs:234` | Profile/directory key checks and env-binding origin checks were incomplete; malformed/stale bindings were not consistently evicted. | Verify key, origin, paths, identity document and shared lock; remove stale or invalid bindings under the binding lock. | `6726a4f`, `2b1acb0` |
| High | `src/selection.rs:422` | Unbound online env pairs always failed; a bound token's changed user could silently select another identity. | Validate online before initializing/binding; reuse validation for doctor; refuse changed users before writing or rebinding. | `6726a4f`, `2b1acb0` |
| Medium | `src/commands/auth.rs:48` | Login ignored `CANVAS_PROFILE`, chose `default_profile` instead of the specified `default` label, and used an env host despite explicit profile selection. | Apply login's profile/input rules, warn when ignoring the env host, and only bind the actual validated env pair. | `6726a4f` |
| Medium | `src/config.rs:175`, `src/config.rs:212`, `src/config.rs:235` | Splitting every underscore broke keys such as `default_profile` and `network.api_concurrency`; writes persisted transient env overrides; color flags were absent from the effective config. | Preserve field underscores, apply flags last, and read persistent-only values for writes. Add precedence and persistence regressions. | `6726a4f`, `2b1acb0` |
| High | `src/config.rs:434`, `src/config.rs:473` | A predictable truncating temporary file could follow a symlink; concurrent config updates lost profiles. | Use unique create-new temporary files, directory fsync on Unix, and a shared config read/modify/replace lock across writers. | `6726a4f` |
| High | `crates/canvas-core/src/store/db.rs:245` | Concurrent WAL initialization could fail with BUSY; the worker masked this with broad retries inside the test. | Restore the strict single-open test and retry only WAL initialization's typed BUSY/LOCKED errors within the existing five-second budget. | `712d6c7` |
| Medium | `src/commands/doctor.rs:12`, `src/commands/doctor.rs:78` | Doctor claimed checks it had not performed, omitted headers, swallowed validation failures, and returned process success with JSON exit 12. | Execute integrity/schema checks, report actual locks, inspect rate-limit/Date headers, preserve validation exits, use a fixed check order, and match process/envelope exits. Connect the named no-op core recovery hook. | `2b1acb0` |
| Medium | `src/exit.rs:105`, `src/token.rs:84`, `src/commands/auth.rs:357` | Aborts emitted no JSON, rate limits mapped to 1, decode failures mapped to network errors, request costs/counts were inaccurate, and non-reveal token output violated its claimed status schema. | Emit one error envelope with available identity/HTTP/request metadata; map exits correctly; use client telemetry and the complete status result. | `2b1acb0` |
| Medium | `src/main.rs:48`, `src/commands/identity.rs:30` | Blocking credential/filesystem operations ran on the current-thread runtime; identity listing hid journal errors and omitted human sizes. | Dispatch through core's blocking pool, retain identity locks throughout listing, propagate journal errors, and show sizes. | `2b1acb0` |
| Medium | `tests/review_credentials.rs:1`, `tests/review_selection.rs:1`, `tests/doctor.rs:1`, `src/credentials.rs:807` | Required crash, dual-deletion-failure, unsafe-file, concurrency, and real keyring coverage was missing or only nominal. | Add assertions of durable flags/token usability, every activation boundary, preservation after failed removal, concurrent bindings/profiles, input precedence, online binding/mismatch, JSON/transport errors, and gated native keyring round-trip. | `3ded806`, `6726a4f`, `2b1acb0` |

Appendix A's existing direct pins and keyring `=4.2.0`, no-default-features, `v1` route remain intact. The hidden reader adds pinned `rpassword =7.4.0`; filesystem checks use pinned `rustix =1.1.4`; the browser offer uses the existing `open =5.4.3` workspace pin. The resulting lockfile passes deny and the native MSRV gate.

API/core changes are limited to the safe key parser correction, a header-preserving accessor on the existing API request path, the explicitly requested no-op journal hook, and the production WAL fix with restoration of the worker-weakened test. No parallel HTTP client, credential schema migration, or replacement identity/store implementation was introduced. Other command packages remain their existing stubs.

## Needs a decision

1. **Persistent validation of env override tokens (§8).** The sole `credential.token_sha256` both authorizes the active stored token and is described as recording successful token validation. If stored token A remains active and env token B validates for the same user, recording B in that field makes A unusable after the environment override disappears. A separate validated-hash record, or an explicit exception to persistent recording for env overrides, needs a spec/schema decision. Current behavior validates doctor network calls and env pairs, rejects identity mismatches, and leaves the active store/hash unchanged. Persistent first-seen validation tracking for a different env token is therefore not implemented; this is not reported as complete.

2. **`doctor --network --offline` (§5 versus §8).** The doctor behavior note says network checks are skipped with `--offline`; the class table makes `doctor --network` class D, whose offline rule requires exit 2 before I/O. The existing exit-2 behavior is retained. Specify which rule wins before changing it.
