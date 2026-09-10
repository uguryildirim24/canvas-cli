# Testing

The workspace runs on `cargo nextest`. Five gates guard every package
(SPEC §16):

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
```

## The end-to-end suite

`crates/canvas-cli/tests/e2e/` runs the shipped `canvas` binary against a
`wiremock` Canvas built from `crates/canvas-api/tests/fixtures/`, and snapshots
stdout, stderr and the exit code with `insta`.

```sh
# the whole suite
cargo nextest run --all-features -E 'binary(e2e)'

# one module, or one test
cargo nextest run --all-features -E 'binary(e2e) and test(commands)'
cargo nextest run --all-features -E 'binary(e2e) and test(exit_9)'
```

No test in the suite is `#[ignore]`d.

Every test builds its own [`E2e`](../crates/canvas-cli/tests/e2e/harness.rs):
a private config root, a private data root, the file credential store, and the
presentation environment SPEC §16 row 3 fixes — `COLUMNS=100`, `--color never`,
`TZ=America/New_York`, and `CANVAS_NOW` frozen at `2026-09-09T17:05:12Z`. The
roots come from `canvas_core::test_scratch::Scratch`, so they are removed when
the test ends and the suite runs in parallel.

`CANVAS_NOW`, `CANVAS_TEST_FORCE_FILE`, `CANVAS_TEST_KEYRING_ERROR`,
`CANVAS_TEST_CRASH_AFTER`, `CANVAS_TEST_ALLOW_HTTP` and `CANVAS_TEST_NO_LAUNCH`
are all gated on `cfg!(debug_assertions)`: a release build ignores them.

### Updating snapshots

Snapshots live in `crates/canvas-cli/tests/e2e/snapshots/`. To review a change:

```sh
cargo insta test --test e2e --review     # or: cargo insta review
```

To write every snapshot without reviewing — only when you have already read the
diff:

```sh
INSTA_UPDATE=always INSTA_FORCE_UPDATE=1 cargo nextest run --all-features -E 'binary(e2e)'
```

Run the suite a second time afterwards with no `INSTA_*` variables. A snapshot
that changes between two runs is carrying a value the harness has not pinned;
add it to `VOLATILE_KEYS`, `VOLATILE_NUMBER_KEYS`, or `E2e::mask` rather than
accepting it. The values already pinned are the journal and receipt UUIDs, the
journal `created_at`/`updated_at` (which `canvas-core` stamps from the wall
clock), the identity database size, the fixture server's port, the crate
version, the build target and commit, and `doctor`'s clock skew.

### Adding a fixture

1. Put the Canvas document in `crates/canvas-api/tests/fixtures/`. It must be
   sanitized: `cargo xtask sanitize` applies the §15 allowlist and strips URLs.
2. Add an accessor to `Fixtures` in `harness.rs` — the `fixture!` macro reads
   the file at compile time, so a typo is a build error.
3. Mount it on a route in `CanvasServer::start`, or, when only one test needs
   it, from that test with `override_get` / `override_post_or_get`. The shipped
   routes all carry the default `wiremock` priority and an override carries a
   higher one, because `wiremock` otherwise answers with the first match in
   mount order.
4. Run the suite with `INSTA_UPDATE=always`, read the new snapshots, then run
   it again unset to confirm they are stable.

Adding a **`result` payload** rather than a Canvas document means a registry
fixture in `crates/canvas-cli/src/output/schemas/` and an entry in
`all_schemas()` in `src/output/registry.rs`. A schema whose Appendix D row
lists more than one shape has one entry per shape, named by `variant`. Add the
file to `SHAPES` in `tests/e2e/schema.rs` too;
`the_shape_table_covers_every_registry_fixture` fails otherwise.

## What the suite covers

| Module | Deliverable |
|---|---|
| `harness.rs` | the fixture server, the isolated environment, the snapshot helper |
| `commands.rs` | every v1 command, table and `--json` |
| `raw_output.rs` | `completions`, `receipts export --out -`, `calendar --ics -`, `auth token --reveal`, `config edit`, and the `--json` usage error on each |
| `exits.rs` | one test per SPEC §14 exit code, 0 through 13 |
| `precedence.rs` | the §14 abort order and the completed-command order |
| `selection.rs` | the §16 row 3 items with no earlier end-to-end test |
| `schema.rs` | every `--json` snapshot against its registry fixture |

### Schema conformance

`schema::every_json_snapshot_matches_its_registry_fixture` reads every JSON
snapshot the suite wrote and checks the `result` against the registry fixture
for its `schema`:

- **field presence** — Appendix D says every listed field is always present, so
  the key sets must match in both directions;
- **nullability** — a fixture value of `null` declares the field nullable;
  `NULLABLE_WITH_EXAMPLE` lists the Appendix D `T?` fields whose fixture shows
  an example instead, and a live `null` anywhere else fails;
- **arrays are never null** — a fixture array is an array in the live payload;
- **sort order** — the Appendix D `Sort` column, per schema.

The §7 envelope rules are checked on the same snapshots: the fixed key set,
ids as strings, and a `<name>_local` sibling for every `ts+local` field.

### SPEC §16 row 3, item by item

The first two items of the row are the whole of `commands.rs` and `exits.rs` /
`precedence.rs`. The eleven named items after them are covered as follows.
`selection::every_row_3_item_names_a_test_that_exists` checks that every test
named here exists.

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
| IPv6 origin identity key on Windows path rules | `identity.rs::ipv6_origin_key`, `e2e/selection.rs::ipv6_origin_identity_key_obeys_windows_path_rules` |

<!-- /spec-16-row-3 -->

### Exit codes and precedence

`exits.rs` holds one test per §14 code and asserts the schema and the outcome
as well as the code, so an abort (the `error` schema) stays distinguishable
from a completed command with a non-success outcome.

The **abort order** — 2 → 3 → 13 → 4 → 5 → 6 → 7 — is an order of detection,
so a pair is only assertable when one invocation can be in both states at once.
`precedence.rs` constructs every such pair. `4` before `5` is the one adjacent
pair no invocation can hold: a request that never connects cannot also come
back rate limited, and the first dataset failure ends the command before a
second route is asked.

The **completed-command order** — 9 > 10 > 8 > 12 > 11 > 0 — has the same
limit and a tighter one: 8 and 11 are terminal for the command that can
produce them, so no invocation carries one of them beside a lower-ranked
outcome. `9 > 12` (a `submit` left `upload_incomplete` by a partial upload)
and `12 > 0` are asserted in `precedence.rs`; `10 > 12` is asserted by
`download.rs::modified_force_mismatch_precedence_and_move_previous_path`;
`canvas-core`'s `download::install::outcome_exit_code` ranks the whole list and
is unit tested against every action.

## The other layers

| Layer | Where |
|---|---|
| `canvas-api` against `wiremock` | `crates/canvas-api/tests/` |
| `canvas-core` unit tests and fixtures | `crates/canvas-core/src/**/tests.rs` |
| `canvas-cli` per-package integration tests | `crates/canvas-cli/tests/*.rs` |
| benchmarks | `cargo xtask bench` |
