# M0-a — Workspace skeleton (Cursor Auto)

Read `docs/SPEC.md` §13 (architecture), §16 (gates), §17 (distribution),
Appendix A (versions). Build the empty-but-green workspace. No feature code.

## Deliverables
1. `Cargo.toml` workspace with members `crates/canvas-api`, `crates/canvas-core`,
   `crates/canvas-cli`, and `xtask`. `resolver = "3"`, `edition = "2024"`, `rust-version = "1.88"`,
   `license = "MIT OR Apache-2.0"`, shared `[workspace.dependencies]` pinned to
   Appendix A versions, `[workspace.lints]`: `rust.unsafe_code = "forbid"`,
   `clippy.all = "warn"`, `clippy.pedantic = { level = "warn", priority = -1 }`,
   allow `module_name_repetitions`, `must_use_candidate`, `missing_errors_doc`,
   `missing_panics_doc`. Every crate sets `[lints] workspace = true`.
   `xtask` is an empty binary crate with a `bench`, `record`, `sanitize`, and
   `dist-assets` subcommand that each print `not implemented yet` and exit 1.
2. `crates/canvas-api`: lib with `pub mod error;` (thiserror enum from SPEC §11,
   variants only, no logic), a `Secret` newtype whose Debug/Display print
   `[redacted]`, and a `Client` struct stub with `new(origin, token: Secret,
   user_agent)` that only stores fields. Depends on reqwest, tokio, serde,
   serde_json, jiff, thiserror, tracing, futures-util.
3. `crates/canvas-core`: lib with empty modules `store`, `identity`, `sync`,
   `resolve`, `todo`, `journal`, `receipts`, `download`, `ics`, `markdown`,
   `io` (blocking bridge), each with a one-line doc comment. Depends on
   canvas-api, rusqlite (bundled), cap-std, cap-fs-ext, serde, serde_json,
   jiff, thiserror, sha2.
4. `crates/canvas-cli`: binary named `canvas` (`[[bin]] name = "canvas"`). clap
   derive with global flags from SPEC §5 (`--json`, `--color`, `--profile`,
   `--fresh`, `--offline`, `-q`, `-v`; `--fresh` and `--offline` use
   `conflicts_with`) and subcommands `version` and `completions <shell>` only.
   Every other subcommand from §5 (including the nested ones: `auth *`,
   `identity *`, `submission verify|reconcile`, `receipts *`, `cache *`,
   `config *`, `alias *`, `open *`) is declared as a stub that prints
   `not implemented yet` to stderr and exits 1 — this locks the command names
   and help text. Depends on clap (derive, env), clap_complete,
   anyhow, anstream, anstyle, tokio (current_thread main).
5. `rust-toolchain.toml` (`channel = "stable"`), plus a `just msrv` recipe that
   runs `cargo +1.88 check --workspace --all-targets` (install the toolchain with
   `rustup toolchain install 1.88` if missing), `deny.toml` (allow MIT,
   Apache-2.0, BSD-2/3, ISC, Unicode-3.0, Zlib, MPL-2.0; deny copyleft; advisories
   deny), `.gitignore`, `README.md` (3 paragraphs from SPEC §0), `LICENSE-MIT`,
   `LICENSE-APACHE`.
6. `justfile` with recipes: `fmt`, `lint` (clippy all targets all features
   -D warnings), `test` (`cargo nextest run --all-features`; fall back to
   `cargo test` if nextest is missing), `deny`, `check` (all four), `build`.
7. `.github/workflows/ci.yml`: on push and PR, macOS + ubuntu matrix, runs the
   four gates with `taiki-e/install-action` for nextest and cargo-deny.
8. `crates/canvas-cli/tests/cli.rs` with `assert_cmd`: `canvas version`
   prints the crate version and exits 0; `canvas todo` exits 1 with
   `not implemented yet`; `canvas --help` lists every §5 v1 command;
   `canvas todo --fresh --offline` exits 2 with a clap usage error.

## Rules
- Owner files: everything under the repo; it is empty. Do not touch `docs/` or
  `tasks/`.
- Work on the current branch `lane/m0a` (already checked out). Commit as you
  go with conventional messages (`chore: workspace skeleton`, `ci: gates`,
  ...). Do not push. Do not merge into `main`.
- `.gitignore` must include `target/`, `.target/`, and `.worktrees/`.
- Use the exact crate versions from SPEC Appendix A. If one does not resolve,
  use the nearest and note it in your final message.
- If `cargo` needs a network you cannot reach, stop and report.

## Gates (all must pass before you report)
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features   # or cargo test --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
```

Finish with `git status --short` and reply exactly: `DONE M0-a`
