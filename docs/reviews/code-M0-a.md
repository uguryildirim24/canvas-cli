MERGE
M0-a's workspace skeleton and registered command surface pass all five required local gates.
The remaining parser defect is fixed and committed; no M0-a decision blocks merging.

## Scope and evidence

Reviewed `tasks/m0a-skeleton.md`, SPEC §§5, 7, 11, 13–17 and Appendix A, `git log main..HEAD --stat`, and the branch diff, including manifests, lockfile, source, tests, licenses, recipes, and CI. Review started at `5afc4e6`; five earlier `review(M0-a):` commits were already present. Those fixes were inspected and revalidated, not recreated. Final reviewed code commit: `609f8b765894f535714c8350a6961dffeb1b20e7` on `lane/m0a`.

This verdict covers the empty-but-green M0-a package. HTTP behavior, identity selection, containment, persistence, JSON schemas/rendering, and the full §16 feature suite belong to later packages under §18. The brief explicitly requires non-version/completion commands to remain stderr-only stubs returning 1.

## Gate results

Every command used `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/reviewer`. Local host: Apple Silicon macOS; stable compiler: Rust 1.97.1; nextest: 0.9.143; cargo-deny: 0.20.2.

| Gate | Final result |
|---|---|
| `cargo fmt --all --check` | PASS, exit 0. |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS, exit 0. |
| `cargo nextest run --all-features` | PASS, exit 0; 20 passed, 0 skipped, no leak warnings. Run ID `81259c79-3a76-4769-b73a-8c02deddea6e`. |
| `cargo deny check` | PASS, exit 0; advisories, bans, licenses, and sources all OK. Nonfatal duplicate-version warnings for `base64`, `syn`, and `windows-sys`. |
| `cargo +1.88 check --workspace --all-targets` | PASS, exit 0. |

The initial 19-test run passed with one nextest `LEAK` classification for `client_stores_fields_without_printing_token`. That test passed without the classification in an isolated rerun and the final complete run. Its cause was not established. Intermediate parser changes failed reserved-name regression tests and were corrected before the final gates.

Also verified all four `xtask` commands (`bench`, `record`, `sanitize`, `dist-assets`): each returned 1, wrote exactly `not implemented yet\n` to stderr, and produced no stdout. `git diff --check` passed. The macOS/Ubuntu CI definition includes the required gates, Appendix A tool pins, Rust 1.98.0, and MSRV checking; hosted CI and release-target builds were not executed during this local review. Unused shared dependencies are declared pins, not evidence that future packages using them compile on MSRV.

## Defects found and fixed

Locations refer to the final files. Rows marked “existing fix” describe commits present at review start; the last row is this review's new fix.

| Severity | File:line | What was wrong | What changed | Commit hash |
|---|---|---|---|---|
| High | `crates/canvas-api/src/error.rs:83` | Derived `Debug` exposed raw forbidden-response bodies and validation messages, potentially including credentials and signed URLs. | Existing fix: safe metadata-only `Debug`; tests cover secret/client formatting and sensitive server content. | `4c7ed59840d642f64c4893197426e123da8d72bc` |
| Medium | `crates/canvas-cli/src/main.rs:175`, `:276`, `:455` | External-subcommand fallbacks accepted malformed operands/unknown flags; split-level global conflicts escaped clap's checks. | Existing fix: explicit mixed operands/subcommands, final conflict validation, and regression tests. | `ca1652c541f33c733eab64d180ce55573e24c7be` |
| Medium | `crates/canvas-cli/src/main.rs:135`, `:149`, `:211`, `:494`; `crates/canvas-cli/tests/cli.rs:35` | Invalid bucket/content/scope combinations passed; download file lists were underspecified; raw-output commands accepted JSON; help tests could pass from the static command appendix alone. | Existing fix: typed choices, exclusive required groups, file-list arity, raw-output conflicts, and tests exercising registered commands, nested help, and all completion shells. | `cfaf3d05d2ceadeaaa59dba1e168f76798720265` |
| Medium | `Cargo.toml:17` | Dependency requirements used compatible ranges instead of the brief's exact pins. | Existing fix: exact shared requirements matching Appendix A's specified versions, with concrete versions for its open patch/latest entries. | `3fc8b9a69b1f29f885d9b22961c0668d5bbd35b6` |
| Medium | `.github/workflows/ci.yml:18` | CI tooling/compiler versions floated and MSRV was not enforced in CI. | Existing fix: compiler/tool pins and an explicit MSRV install/check. | `5afc4e694890fa63f0c25cd7d3f4098e21761caa` |
| Medium | `crates/canvas-cli/src/main.rs:180`, `:481`; `crates/canvas-cli/tests/cli.rs:183` | `submission chem --fresh 123` and equivalent interspersed options failed with exit 2 because clap required two contiguous values. | Allow one or two values per contiguous group, then require exactly two operands overall. Added regressions for all global flags, history, missing/extra operands, conflicts, and escaped reserved names. | `609f8b765894f535714c8350a6961dffeb1b20e7` |

## Security and contract checks

All §11 error variants are present with the specified payloads; `Secret` formats as `[redacted]`, and error diagnostics omit response content. The client only stores fields. Source inspection found no implemented network, credential-store, or raw-body persistence paths. Core modules are empty and documented at their declarations; capability containment is therefore deferred, not claimed as tested.

All required M0-a CLI tests are present and pass, including actual command registration, exit codes 0/1/2, global conflicts, and nested stubs. A positional value matching a subcommand after an option uses clap's `--` escape, e.g. `submission chem --fresh -- verify`. Module layout, inherited lints, current-thread runtime, bundled SQLite, exact specified dependency versions, ignore entries, licenses, and required recipes conform to the brief. The explicit brief's license allowlist, including MPL-2.0, was retained.

## Needs a decision

None for M0-a. No spec changes were needed. No push or merge was performed. The pre-existing untracked `tasks/review-code-m0a.md` was left untouched.
