MERGE
The command and transport defects found in this review are fixed and committed; all five final gates pass.
Verified with local HTTP fixtures, real destination directories, and subprocesses; the pending R4 interfaces remain deferred below.

## Scope

Reviewed `tasks/m3b-download-command.md`, its cited SPEC sections (§§5, 7, 9–11, 12.3, 14–16 and Appendices A/D), `docs/reviews/code-M3-b-core.md`, the worker log, and the complete worker diff (`7fa48f8`). The required initial `git merge main` completed as `e7d9f4f`; no subsequent merge or push was performed. Final reviewed implementation and tests: `23f40ce` on `lane/w2`.

This report is the explicit review-task exception to the original worker brief's prohibition on writing docs. No SPEC, task, migration, dependency pin, or shared R4 registry/enum implementation was changed during the review.

## Gates

Recorded final gate commands used `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/rev-m3b-cmd`.

| Gate | Final result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS, Rust 1.97.1 |
| `cargo nextest run --all-features` | PASS: 328 passed, 0 skipped, no leaky tests; run `e1a7f1c0-f592-42dc-9d98-85bddb293de9` |
| `cargo deny check` | PASS: advisories, bans, licenses, sources; non-fatal duplicate-version warnings |
| `cargo +1.88 check --workspace --all-targets` | PASS, compiler verified as Rust 1.88.0 |

The initial five gates also passed (308 tests). They did not expose the defects below. One intermediate run reported a leaky HTTP-fixture test; the complete final run did not. Snapshot trailing spaces are the renderer's existing table padding and are preserved intentionally.

## Defects found and fixed

Locations identify the original worker tree unless otherwise stated. `cli/`, `core/`, and `api/` abbreviate `crates/canvas-cli/`, `crates/canvas-core/`, and `crates/canvas-api/`.

| Severity | File:line | What was wrong | What changed | Fix commit |
|---|---|---|---|---|
| High | `api/src/request.rs:303` | The HTTP fixture override allowed arbitrary plaintext transfer hosts, including in release builds. | Restricted it to debug builds and literal loopback hosts; HTTPS and credential-free URLs remain required elsewhere. Added a command regression that refuses a non-loopback HTTP capability URL without exposing its query. | `289a5b2`, `23f40ce` |
| High | `core/src/download/transport.rs:74,97` | A second metadata request could install bytes against stale classification metadata; the URL-refresh branch ignored a newly locked file and revision changes. | Reuse the exact initial metadata, refresh once, recheck access and size/timestamp consistency, and fail a changed revision. Capability URLs remain in memory. | `289a5b2`, `23f40ce` |
| High | `core/src/download/transport.rs:31` | Availability dates were ignored; raw `locked` could override an explicit effective-access value. | Respect effective access and remote unlock/lock dates on initial metadata and refresh. | `289a5b2`, `9e490be`, `23f40ce` |
| Medium | `core/src/download/transport.rs:33`, `cli/src/commands/download.rs:348` | Zero and unknown size were conflated; empty files were rejected and stale listing hints substituted for fresh metadata. | Preserve zero as a valid size; reject missing size and mismatched file IDs; use fresh metadata for transfer, classification, and reported size. | `289a5b2`, `5f50cab`, `23f40ce` |
| High | `cli/src/commands/download.rs:334,214`, `core/src/download/transport.rs:136` | API errors became strings and authentication/network/throttle aborts became ordinary per-file partial results; later aborts could discard already installed results. | Preserve typed API errors, map required abort codes, stop scheduling new work after an abort, drain in-flight work, and include per-file results in `error@1.details.download`. Prepare discovery before installing. | `289a5b2`, `5f50cab`, `23f40ce` |
| Medium | `cli/src/commands/download.rs:291,382,410` | Verification used `outcome: error` with a download payload, blocked the async executor while hashing, and converted filesystem/DB errors into mismatches. | Emit `outcome: mismatch` for exit 10; hash in blocking work under install locks; retain typed persistence/containment failures instead of inventing a hash mismatch. | `5f50cab`, `23f40ce` |
| High | `cli/src/commands/download.rs:171,199` | Recovery actions disappeared under filters, used the current planned path for both old and new files, omitted leftover warnings, and suppressed normal work after the only-old-match branch. Ordinary moves omitted `previous_path`. | Retain original recovery rows while locked, attribute actual paths/courses, report destination-wide recovery independently of filters, continue benign recovery through normal planning, and supply previous paths. Unresolved/unsafe recovery remains a per-file refusal. | `5f50cab`, `23f40ce` |
| Medium | `cli/src/commands/download.rs:637,641,645` | Listing denials and resolution freshness were dropped, allowing incomplete discovery to look successful. | Preserve course/dataset freshness and stale warnings; report independent folders/files denials in `partial[]` and warnings, with exit 12 except during dry-run. | `5f50cab`, `23f40ce` |
| Medium | `cli/src/commands/download.rs:680,687` | A module filter tested the owning module instead of membership, dropping files linked from another matching module and all external-tool items. | Filter the completed whole-course plan using matching modules' file/external IDs, retaining the original owner and collision suffixes. | `5f50cab`, `23f40ce` |
| Medium | `cli/src/commands/download.rs:211,517` | Per-file bars had no byte callback; dry-run byte totals were zero; human totals omitted most actions and verification/error details. | Connect cumulative transfer progress to file and total bars; sum planned bytes; render all action counts, verification, previous paths, and per-file errors. Quiet/JSON continue to suppress bars. | `5f50cab`, `23f40ce` |
| High | `core/src/io/sqlite.rs:15`, `cli/src/commands/download.rs:127` | The core review's deferred executor composition was still unresolved: invoking downloads alongside the store created two SQLite threads. Download connections also remained cached beyond the caller's identity-lock lifetime. | Route download jobs through the store's existing bounded worker. Open/close auxiliary connections within each job, set the five-second busy timeout, and test identical worker thread IDs and panic isolation. No store migrations changed. | `183f772` |
| Medium | `cli/tests/download.rs:42,85,208,566` | Required adversarial command cases were missing or weak. Stale fixtures silently exercised denied listings rather than successful discovery, and the purported dot-dot test only created a final symlink. | Freeze fixture time/zone/width, keep successful discovery fresh, strengthen identity/rerun assertions, correct test naming/snapshots, and add the coverage listed below. | `5f50cab`, `23f40ce` |

## Verification coverage

- Real command invocations cover empty/unknown-size files; an initial download and zero-transfer rerun; unmanaged/modified files and forced replacement; owned remote revisions; rename moves and occupied targets; mismatch precedence over locked/partial results; metadata authentication abort with prior installed results; listing denial reporting; and all-courses downloads.
- Separate API/storage origins verify one expired-URL refresh, repeated expiration failure, refreshed lock/revision refusal, successful refresh, off-origin bearer stripping, identity encoding, and absence of capability queries in output. An incomplete transfer leaves no final/part file; the next invocation restarts without a Range header.
- Containment tests use real directories for symlinked parents/finals and deterministic final/parent swaps during HTTP transfer with `--force --verify`. Recovery fixtures inject absolute and parent-traversal paths. Existing core tests additionally exercise the inspection-to-open swap and component sanitization. Outside sentinels remain unchanged.
- Two command subprocesses reach storage before installation, then produce exactly one downloaded and one skipped result; a verified rerun remains skipped. Identity mismatch is checked before destination registry/manifest creation; orphan and damaged metadata refuse initialization.
- Recovery tests cover new/old/both/neither matching and reporting outside file filters. Collision tests retain suffixes and owner paths under module/file filters, including a non-owning module and external-tool counts. Dry-run creates neither destination metadata nor an identity-side download manifest.
- Unit tests cover every action count and completed-exit precedence, incremental progress callbacks, metadata validation, and the shared SQLite worker. Human and JSON snapshots pass. Progress state was tested programmatically; no interactive TTY appearance claim is made.
- Appendix A pins remain unchanged, including `indicatif =0.18.6`, `cap-std/cap-fs-ext =3.4.6`, and `fs4 =0.13.1`. No live Canvas account, native Windows execution, or actual cross-filesystem move was used. Existing core fixture tests cover fingerprint refusal, registration/recovery, and lock behavior on this host.

## Deferred to R4 interfaces

These are integration obligations, not defects charged to this package, as directed by `tasks/review-code-m3b-cmd.md`:

- Lane w1's final `download@1` registry/types/fixture: reconcile the worker's provisional shared registry definitions with the landed interface. Its current `skip_serializing_if` behavior still omits absent `previous_path`, `size`, `error`, and `verify`; R4 must supply the SPEC's always-present nullable fields and matching fixtures/schema checks. This review does not claim final R4 schema conformance.
- Lane w3's final command enum/dispatch interface: reconcile the worker's existing `main.rs` dispatch with the final enum using the command module's `DownloadArgs`/`run` entry point. Existing parser/dispatch behavior was exercised, but the pending R4 interface was not invented or merged here.
- Rerun the shared interface/schema/parser gates when those interfaces land. The already-present destinations migration and the store/manifest worker composition are exercised in this review; they are no longer deferred.

## Needs a decision

None. The fixes follow the current SPEC; no specification change or owner decision was required.
