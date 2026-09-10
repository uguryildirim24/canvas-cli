# M5-c — End-to-end snapshot suite and exit-code precedence (Cursor Auto, lane w3)

Read `docs/SPEC.md` §5 (every command), §7 (human and `--json` contracts,
raw-output rule, single-document rule), §8 (selection matrix, offline
matrix, credential protocols), §14 (every code and the precedence rule),
§16 row 3 (**the complete list**; each item becomes at least one test),
Appendix D (every schema). Existing code: `crates/canvas-cli/tests/` (per-
package tests from M0-c through R4), the schema registry and its fixture
test, `wiremock` helpers, `insta` snapshots. Read them first and reuse the
helpers; do not duplicate a case that an earlier package already covers
with the same assertion (extend it instead and say so).

## Deliverables
1. A test harness `crates/canvas-cli/tests/e2e/` with: a `wiremock` Canvas
   fixture server built from `crates/canvas-api/tests/fixtures/`, an
   isolated `XDG`/config/data root per test, a fake credential store (file
   fallback) and a fake keyring failure mode, `COLUMNS=100`, `--color
   never`, fixed `TZ=America/New_York`, frozen `CANVAS_NOW` (test builds
   only; confirm the cfg gate from M0-b/M1-b), and a helper that runs
   `canvas` via `assert_cmd` and snapshots stdout, stderr, and the exit code
   with `insta`.
2. Every v1 command in the §5 block, in table mode and `--json` mode, with
   a snapshot each (`insta`), including the raw-output commands
   (`completions`, `receipts export --out -`, `calendar --ics -`) and the
   `--json` usage error on them.
3. Every exit code in §14 with at least one test, and the precedence rule:
   abort order 2 → 3 → 13 → 4 → 5 → 6 → 7 (construct each pair where two
   apply and assert the earlier one wins), completed-command order
   9 > 10 > 8 > 12 > 11 > 0 (e.g. `download --verify` with a mismatch and a
   failed file exits 10; `submit` that ends `outcome_unknown` with a partial
   upload exits 9).
4. The §16 row 3 list, item by item: account switch refused; env-pair
   identity offline exit 3 then online validation and binding-file lookup;
   `auth login --profile NEW`; credential activation crash between each
   step (stray, pending cleanup, recovery on next login/logout); repeated
   fallback login with the keyring still unavailable; logout with two
   deletion failures leaves `active_source = none` and both flags;
   resolution rejects `none`; concurrent env-binding writes; class-B
   command with an unbound env pair and no default profile; `identity
   remove <key>` with no default profile; IPv6 origin identity key on
   Windows path rules (unit-level on the key function, plus a `cfg(windows)`
   path test).
5. A schema conformance test: every `--json` snapshot is validated against
   the registry entry (field presence, nullability, array-never-null, sort
   order) using the fixture shapes from Appendix D.
6. `docs/testing.md` (the one file under `docs/` you may write): how to run
   the suite, update snapshots, add a fixture, and the list of §16 row 3
   items with the test name that covers each.

## Rules
- You own `crates/canvas-cli/tests/e2e/**` and `docs/testing.md`. You are
  the **enum owner** and **registry owner** this round: no additions are
  expected; if a test reveals a missing variant or entry, add it and say so.
  Fix a product defect a test exposes only when the fix is local and
  obvious; otherwise record it under "Defects found" in your final message
  and mark the test `#[ignore]` with the reason.
- Work on branch `lane/w3` in this worktree. Commit as you go. Before
  reporting, `git merge main` (resolve, rerun gates). Do not push.
- Do not touch `docs/` (except `docs/testing.md`) or `tasks/`.

## Gates
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
```
Finish with `git status --short`, the list of `#[ignore]`d tests (if any),
and reply exactly: `DONE M5-c`
