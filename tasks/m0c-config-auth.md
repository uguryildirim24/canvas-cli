# M0-c — Config, identity selection, credentials, `auth *`, `identity *`, `doctor` (Cursor Auto, lane w1)

Read `docs/SPEC.md` §5 (command classes, behaviour notes for `auth login`,
`identity *`, `doctor`), §8 (all of it), §9 (paths, config.toml), §10
"Databases and locking" and "Identity removal", §13, §14, §16 row 3 (the
auth/identity cases), Appendix A, Appendix D (`auth_status@1`,
`auth_login@1`, `auth_logout@1`, `identity@1`, `config@1`, `doctor@1`).
Existing code you build on: `canvas-api` (`Client`, `Secret`, `get`),
`canvas-core::identity` (identity key, `identity.json`, identity locks,
removal protocol), `canvas-core::store` (`credential` row, `Store::open`).
Read their public APIs before writing anything; do not fork them.

## Deliverables
1. `crates/canvas-cli/src/paths.rs`: `Paths` from `etcetera` per §9 (macOS
   uses XDG), `CANVAS_CONFIG_DIR`/`CANVAS_DATA_DIR` overrides for tests.
2. `crates/canvas-cli/src/config.rs`: `figment` layering defaults →
   `config.toml` → `CANVAS_*` env → flags; typed `Config` with profiles
   (origin, user_id, key, name, time_zone), download, cache TTLs, network,
   output; `config path|get|set|edit` with key validation.
3. `crates/canvas-cli/src/selection.rs`: the §8 selection matrix per command
   class (A/B/C/D), env pair handling, env binding file with lock and atomic
   replace, `identity remove <key>` operand rule, `auth login --profile NEW`.
4. `crates/canvas-cli/src/credentials.rs`: keyring 4.2.0 `v1` route, error
   mapping via `Entry::store_status`, never `Debug` keyring errors, fallback
   file protocol (`O_NOFOLLOW` + `fstat` checks, locked read-modify-replace,
   `ENOENT` creates, unsafe existing file refuses), Windows: no fallback.
   Activation and logout protocols with `active_source ∈ {keyring, file,
   none}` and the two cleanup flags exactly as §8 states; per-identity
   `.cred.lock`.
5. Token resolution and validation (§8 table): `CANVAS_TOKEN` → active
   source with hash check; `GET /users/self` outcomes → exits 3/4/13;
   `identity mismatch` never joins data.
6. Commands: `auth login` (canonical origin, prompt/`--token-stdin`/
   `CANVAS_TOKEN`, validation, store, profile create/update, `--replace`),
   `auth status`, `auth logout`, `auth token --reveal` (raw output, refuses
   `--json`), `identity list`, `identity remove` (confirmation, exclusive
   lock, credentials → directory → profiles), `doctor` (class B with
   identity-free fallback; local checks; `--network` checks; owner-absent
   journal recovery hook is a no-op call into core until M2-a lands: leave a
   clearly named extension point).
7. JSON: render through the envelope module if M1-b has landed on `main`
   when you start (check `crates/canvas-cli/src/output/`); otherwise emit
   the exact Appendix D shapes with a minimal local envelope builder in
   `output/envelope.rs` that M1-b will replace. Coordinate by keeping the
   builder's signature `Envelope::new(schema, profile, identity)`.
8. Tests (`crates/canvas-cli/tests/auth.rs`, `identity.rs`, `doctor.rs`,
   with `wiremock` + `assert_cmd`, temp config/data dirs): login stores
   under identity in one store; stray detection; `--replace`; second user
   refused; env-pair matrix incl. offline exit 3 and binding-file lookup;
   `auth login --profile NEW`; activation crash between each step (inject
   via env var `CANVAS_TEST_CRASH_AFTER=<step>`); login after failed logout
   does not delete the new token; logout with two deletion failures;
   resolution rejects `none`; class-B command with unbound env pair and no
   default profile; `identity remove <key>` with no default profile;
   removal with a waiting opener; IPv6 origin key; fallback file unsafe
   (symlink, wrong mode, wrong owner) refused; keyring set/get/delete on
   macOS behind `#[cfg(target_os = "macos")]` + env gate
   `CANVAS_TEST_KEYRING=1`.

## Rules
- You own `crates/canvas-cli/**` this round except `src/output/**` (M1-b
  owns it) and the `courses`/`course`/`alias`/`sync` command modules. You
  are the **command enum owner**: other lanes will ask for enum changes
  through Claude; make the dispatch table easy to extend (one module per
  command group).
- Work on branch `lane/w1`. Commit as you go with conventional messages.
  Before reporting, `git merge main` (resolve conflicts, rerun gates). Do
  not push. Do not merge into `main`.
- Do not touch `docs/` or `tasks/`. Do not modify `canvas-api` or
  `canvas-core` except to add a `pub` accessor you strictly need (note it).

## Gates (all must pass before you report)
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
```
Finish with `git status --short` and reply exactly: `DONE M0-c`
