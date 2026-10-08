# Testing

Run the existing checks from the repository root:

```sh
just check
npm --prefix extension test
```

`just check` runs formatting, Clippy, tests, and cargo-deny in that order.
It stops at the first failure. Tests use cargo-nextest when installed and
otherwise use `cargo test --all-features`.

Rustfmt and Clippy are Rust toolchain components. Just, cargo-nextest, and
cargo-deny are separate tools. Node.js 24 or newer runs the companion checks.
The companion needs no npm dependencies.

## Scope review, October 8, 2026

Runtime Rust code, dependency manifests, Cargo.lock, and the justfile match
HEAD. Privacy replacements remain in fixtures, existing test inputs and
expectations, result examples, and dependent snapshots. No tests were added.
Historical research, measurements, and review evidence remain. Private paths
were removed from review reports. Deleted task briefs and dialogue turns are
agent handoff notes, not runtime features.

The historical consolidation report, specification review ledger, and packaging
results were restored rather than treated as junk. They remain historical
evidence, not current setup instructions.

The section 16 coverage table below was restored. The reply guide and its
existing assertion now name Rolf and retain the authorization phrases. Six
formatting-only JSON changes were reverted. The original error code, cache
table name, top-frame ID, and hostile approval punctuation were also restored.

Checks ran on macOS. Rustc reported 1.97.1 and Node.js reported 24.19.0.
Cargo reused the worktree-local dependency cache with offline access. This was
not a clean machine or a fresh dependency download.

| Check | Observed result |
|---|---|
| `cargo build --locked --release` | Passed with offline dependency access. |
| Release `--help`, `version`, `schema --list`, and `doctor` | Passed with isolated local configuration. Doctor selected no identity and skipped account checks. |
| Credential-free `auth status` and offline `todo` | Exit 3, as expected. |
| Bash, Fish, and Zsh completion generation | Passed into an isolated HOME. Zsh `compinit` also passed. Fish was not loaded in an interactive shell. |
| Earlier `just check` | Formatting passed. Clippy stopped at `assert_is_empty` warnings. No lint suppression or assertion rewrite was added. |
| Final worktree `just check` | Formatting and Clippy passed. Nextest stopped after 471 of 923 tests: 466 passed and 5 socket-path failures. Exit 100. Another 452 tests were not run because of fail-fast. Cargo-deny was not reached. |
| First full nextest run without updates | 923 tests ran: 903 passed, 20 failed. Nineteen failures were socket-dependent. The cache table snapshot also failed. |
| Focused rerun after the snapshot repair | The three existing coverage, reply-guide, and cache checks passed without update variables. The filter omitted 920 checks. |
| Final worktree `cargo nextest run --all-features --no-fail-fast` | 923 tests ran: 904 passed, 19 socket-dependent failures, none skipped. Nextest marked the existing crash-helper check leaky. The repaired checks and registry snapshots passed. |
| Existing skill and identity-selection checks after the final documentation edits | All 15 selected checks passed. Other binaries were not selected. |
| `npm --prefix extension test` | All 52 checks passed. |
| `cargo +1.88 check --offline --locked --workspace --all-targets` | Passed using the already installed toolchain. |
| `cargo deny --offline --locked check` | Incomplete. The local advisory database was absent. No network Git was used. |
| `cargo deny --offline --locked check licenses bans sources` | Passed with duplicate-version warnings. This is not an advisory check. |

The cache table snapshot was first regenerated from the synthetic fixtures.
Its isolated rerun passed, but the full run produced a different SQLite file
size: 155648 rather than 163840 bytes. The existing text scrubber now also
masks `size_bytes: ` with its existing numeric mask. JSON snapshots already
mask this filesystem measurement. This one-line test-only fix is needed for
the README's test step. Runtime output is unchanged. The existing snapshot was
regenerated again and verified without updates. Row counts and dataset fields
remain checked.

The original short-deadline submission check passed in the final full run.
Its HEAD timeout is unchanged. The suite is not fully passing. Earlier results
for a refactored session or changed timeouts do not describe this tree.

## Isolation and socket paths

Keep test HOME, config, data, and temporary roots separate from an authenticated
account. Do not supply a global `CANVAS_CONFIG_DIR` that masks a test's own XDG
override. Cargo output and review-generated files live under ignored `target/`.

The approved worktree path is 108 bytes before any temporary identity path is
appended. The approved review root is also too long for the broker's 103-byte
Unix socket bound. No short checkout or temporary root outside those locations
was authorized. The requested short-root rerun could not be performed without
breaking the path restriction. This environmental verification blocker remains.
Rolf must approve a short checkout and short private temporary root before a
complete rerun. Runtime behavior and endpoint checks were not changed. Do not
skip socket checks and call that a full pass. Windows uses named pipes and was
not checked here.

Coursework sessions use `CANVAS_DATA_ROOT` and only `CANVAS_TOKEN` for online
requests. Auth and identity management use `CANVAS_DATA_DIR` and can read saved
credentials. Set both data roots to the same private directory when isolating
all command classes. See [README.md](../README.md).

## SPEC section 16, row 3 coverage

The existing selection check reads this table and verifies the named tests.

<!-- spec-16-row-3 -->

| Item | Test |
|---|---|
| account switch refused | `auth.rs::replace_rebinds_profile_second_user_refused`, `doctor.rs::already_bound_env_token_cannot_silently_change_users` |
| env-pair identity offline exit 3, then online validation and binding-file lookup | `auth.rs::env_pair_offline_class_b_exit_3_then_binding_after_login` |
| `auth login --profile NEW` | `auth.rs::login_profile_new` |
| credential activation crash between each step | `auth.rs::crash_after_store_write_leaves_stray_then_status`, `review_credentials.rs::rotation_crash_reports_stray_then_pending_and_recovers` |
| repeated fallback login with the keyring still unavailable | `review_credentials.rs::unavailable_backend_and_two_failed_deletions_keep_durable_flags` |
| logout with two deletion failures leaves `active_source = none` and both flags | `auth.rs::logout_two_deletion_failures_leave_flags` |
| resolution rejects `none` | `auth.rs::resolution_rejects_none` |
| concurrent env-binding writes | `review_selection.rs::concurrent_binding_writes_and_logins_preserve_both_bindings` |
| class-B command with an unbound env pair and no default profile | `e2e/selection.rs::class_b_command_with_an_unbound_env_pair_and_no_default_profile` |
| `identity remove <key>` with no default profile | `identity.rs::remove_with_no_default_profile` |
| IPv6 origin identity key on Windows path rules | `identity.rs::ipv6_origin_key`, `e2e/selection.rs::ipv6_origin_identity_key_obeys_windows_path_rules`, `e2e/selection.rs::ipv6_origin_identity_key_is_a_real_windows_directory` |

<!-- /spec-16-row-3 -->

## Fixtures and snapshots

The [fixture README](../crates/canvas-api/tests/fixtures/README.md) describes the
invented model data and result examples. The separate benchmark manifest
already identifies its dataset as synthetic. The
[loopback TLS fixture](../crates/canvas-api/src/transfer_tests/README.md) is a
local test key, not an account credential.

The existing end-to-end suite runs the binary against a mock Canvas server.
Snapshots record mock stdout, stderr, and exits. They are not authenticated
coursework results. All retained snapshots were exercised in the final full
run without update variables.

To inspect the existing end-to-end checks:

```sh
cargo nextest run --all-features -E 'binary(e2e)'
```

For an intentional fixture change, regenerate with the existing tests:

```sh
INSTA_UPDATE=always INSTA_FORCE_UPDATE=1 cargo nextest run --all-features -E 'binary(e2e)'
```

Review every changed field, then rerun without the update variables. Do not
accept live account data into a snapshot. `submit@1` and `operation@1` retain
their original `replayed` field. No output-contract version bump remains.

`CANVAS_NOW`, `CANVAS_TEST_FORCE_FILE`, `CANVAS_TEST_KEYRING_ERROR`,
`CANVAS_TEST_CRASH_AFTER`, `CANVAS_TEST_ALLOW_HTTP`, and `CANVAS_TEST_NO_LAUNCH`
are existing debug-build controls. Release builds ignore them.

## Not checked here

Prerequisite installers were not rerun because the tools were already present
and installations would write outside the permitted worktree. Real Canvas
login, token entry, authenticated coursework, Chrome installation, paid
services, cross-platform builds, and release publication were not run. No crawl
or live fixture recording was started. No new standalone benchmark was run;
the existing full suite did include its local benchmark check.

An earlier publication-file scan covered 728 files. Its matches were public
project metadata, historical institution discussion, and synthetic machine
paths. The final scan covered 729 publication text files. No listed personal
terms, private machine paths, historical account identifier, or former quiz
validation token matched. This was a targeted scan, not a complete
secret-scanner audit.

History cleanup is separate. A tree scan does not remove historical private
identifiers. Rolf must revoke any former example credential that was real and
review the history intended for publication.
