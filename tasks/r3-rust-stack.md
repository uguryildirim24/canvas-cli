# R3 — Rust CLI stack as of September 2026 (research only, no code)

Goal: the recommended crate stack for a cross-platform (macOS first, Linux,
Windows) Rust CLI that talks to a REST+GraphQL API, stores a secret token,
downloads files concurrently, and prints tables or JSON. Use Google, crates.io,
lib.rs, docs.rs, GitHub. Report CURRENT stable versions (verify on crates.io).

Write findings to `docs/research/r3-rust-stack.md` in this repo. Markdown: one
table `crate | version | purpose | why this one | alternatives rejected`, then
a short recommended `[dependencies]` block, then notes. Cite URLs. Mark
unverified items UNVERIFIED. Do not write any code files or run cargo.

Cover:
1. Toolchain: current stable Rust version, edition 2024 status, MSRV advice.
2. CLI parsing: clap (derive), clap_complete (shell completions), clap_mangen.
3. HTTP: reqwest (rustls, json, stream, gzip) vs ureq; tokio runtime; guidance
   for a CLI that does a few concurrent downloads (tokio multi-thread vs
   current-thread, `futures::stream::buffer_unordered`).
4. Data: serde, serde_json; date-time crate choice in 2026 (jiff vs chrono vs
   time) for ISO 8601 parsing + local time-zone display.
5. Secrets: keyring crate (macOS Keychain / secret-service / Windows Credential
   Manager) current version, known pain points (Linux headless, CI), fallback to
   a 0600 file.
6. Config and paths: directories / etcetera / dirs, toml, figment or config.
7. Output: comfy-table vs tabled; anstyle / owo-colors; NO_COLOR and
   `--color=auto|always|never`; indicatif progress bars; `open` crate.
8. Local cache: rusqlite (bundled) vs sled vs plain JSON files; recommendation
   for caching courses/assignments with ETags/timestamps.
9. Errors: anyhow, thiserror, miette, color-eyre — pick one combo.
10. Testing: wiremock vs httpmock; assert_cmd, predicates, insta snapshots;
    cargo-nextest.
11. Optional TUI: ratatui + crossterm versions; a "dashboard" view only.
12. Distribution: cargo-dist (current status/name), Homebrew tap, cargo-binstall,
    GitHub Releases, macOS universal binary, code signing/notarization
    requirements in 2026; release-plz or cargo-release.
13. Quality: cargo-deny, clippy pedantic groups, cargo-audit.
14. Name any Rust crate on crates.io that already wraps the Canvas LMS API.

Finish by replying in chat with exactly: `DONE docs/research/r3-rust-stack.md`.
