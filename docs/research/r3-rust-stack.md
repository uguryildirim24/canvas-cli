# R3 — Recommended Rust CLI Stack (September 2026)

This document specifies the recommended Rust crate stack for `canvas-cli`: a cross-platform (macOS-first, Linux, Windows) command-line interface that interacts with the Canvas LMS REST and GraphQL APIs, securely manages user authentication tokens, executes concurrent file downloads, caches data locally with HTTP validation headers, and renders formatted tables, JSON, and progress indicators.

All crate versions were verified against the live [crates.io](https://crates.io) registry on **September 9, 2026**.

---

## 1. Master Crate Selection Table

| Crate | Version | Purpose | Why This One | Alternatives Rejected |
| :--- | :--- | :--- | :--- | :--- |
| [`clap`](https://crates.io/crates/clap) | `4.6.6` | Command-line argument parsing | De facto Rust standard. Strongly-typed derive macros, subcommands, env var bindings, shell completions integration, and zero-allocation ANSI styling. | `lexopt` (too low-level, manual token parsing), `argh` (lacks completions/manpage generation), `bpaf` (steep combinator learning curve). |
| [`clap_complete`](https://crates.io/crates/clap_complete) | `4.6.9` | Shell completion script generation | Generates completion scripts for Bash, Zsh, Fish, PowerShell, and Elvish directly from `clap` command definitions. | Handcrafted completion scripts (high maintenance, drift from CLI flags). |
| [`clap_mangen`](https://crates.io/crates/clap_mangen) | `0.3.3` | Unix man page generation | Automatically compiles roff man pages directly from `clap::Command` metadata for Homebrew and Linux packaging. | `help2man` / `mdbook-man` (require external runtime dependencies). |
| [`reqwest`](https://crates.io/crates/reqwest) | `0.13.5` | HTTP REST and GraphQL client | Native Tokio async integration, streaming downloads directly to disk, connection pooling, Brotli/Gzip decompression, and `rustls` using platform verification. | `ureq` (v3.4.1 added async, but lacks reqwest's deep Tokio streaming ecosystem and wiremock parity), `hyper` (too low-level). |
| [`tokio`](https://crates.io/crates/tokio) | `1.53.1` | Asynchronous runtime | Production-standard async runtime. Non-blocking network reactor and cooperative thread pool for background file I/O. | `async-std` (unmaintained, ecosystem abandoned), `smol` (smaller ecosystem, lacks `reqwest` integration). |
| [`futures-util`](https://crates.io/crates/futures-util) | `0.3.34` | Async stream concurrency control | Provides `StreamExt::buffer_unordered` to enforce concurrency limits on parallel file downloads without unbounded task spawning. | Unbounded `tokio::spawn` loops (risk OS file-descriptor exhaustion and Canvas API 403 rate-limiting). |
| [`serde`](https://crates.io/crates/serde) | `1.0.229` | Serialization framework | Universal Rust serialization ecosystem. Zero-cost derive macros for request and response structs. | `nanoserde` / `miniserde` (limited type support, missing ecosystem integrations). |
| [`serde_json`](https://crates.io/crates/serde_json) | `1.0.151` | JSON serialization and deserialization | Standard for Canvas REST API payloads, dynamic `serde_json::Value` queries, and `--json` CLI output. | `simd-json` (complex SIMD instruction requirements, negligible benefit for CLI sizes). |
| [`jiff`](https://crates.io/crates/jiff) | `0.2.35` | Date-time handling and ISO 8601 parsing | Modern TC39 Temporal design by BurntSushi. Safe ISO 8601 parsing, integrated IANA timezone database, safe system local timezone resolution, and Serde support. | `chrono` (v0.4.45, legacy design, historical soundness vulnerabilities), `time` (v0.3.55, awkward timezone ergonomics). |
| [`keyring`](https://crates.io/crates/keyring) | `4.2.0` | Secure OS credential storage | Cross-platform secrets storage using macOS Keychain, Windows Credential Manager, and Linux Secret Service. | Unencrypted configuration files, plain environment variables. |
| [`directories`](https://crates.io/crates/directories) | `6.0.0` | Standard OS filesystem locations | Returns standard paths for config, cache, and data across macOS (`~/Library/...`), Linux (`~/.config`, `~/.cache`), and Windows (`AppData`). | `dirs` (v7.0.0, lower-level, requires manual path joins), `etcetera` (v0.11.0, less standard on macOS). |
| [`figment`](https://crates.io/crates/figment) | `0.10.19` | Layered configuration management | Hierarchical configuration merging: Default values -> `config.toml` -> Environment variables -> CLI flags. Emits clean syntax errors with file spans. | `config` (v0.15.25, looser typing, less ergonomic error reporting), raw manual TOML parsing. |
| [`toml`](https://crates.io/crates/toml) | `1.1.5` | TOML serialization and parsing | Human-readable configuration format standard in the Rust ecosystem (`config.toml`). | `serde_yaml` (unmaintained/deprecated), `serde_json` (lacks human comments). |
| [`comfy-table`](https://crates.io/crates/comfy-table) | `8.0.0` | Terminal table formatting | Dynamic terminal width detection via crossterm, automatic content wrapping for long text, UTF-8 borders, and ANSI color support. | `tabled` (v0.22.0, powerful derive model but requires manual wrap modifiers for dynamic terminal widths). |
| [`anstyle`](https://crates.io/crates/anstyle) | `1.0.14` | ANSI text styling primitives | Zero-allocation styling primitives used natively by `clap` 4.x. Shared across the modern Rust CLI stack without extra dependencies. | `owo-colors` (v4.4.0, feature-rich but duplicates styling engines), `colored` (unmaintained). |
| [`anstream`](https://crates.io/crates/anstream) | `1.0.0` | Color stream auto-negotiation | Drop-in stdout/stderr stream wrapper. Automatically respects `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, and `--color=auto\|always\|never`. | Custom tty checks and manual regex escape stripping. |
| [`indicatif`](https://crates.io/crates/indicatif) | `0.18.6` | Progress bars and spinners | Industry standard. Supports multi-bar concurrent downloads, byte counters, transfer rates, ETA, and auto-hiding when piped. | `pbr` (unmaintained), `prodash` (excessively complex for CLI downloads). |
| [`open`](https://crates.io/crates/open) | `5.4.3` | Cross-platform URL and file opener | Dispatches to macOS `open`, Linux `xdg-open`, and Windows `start` to open Canvas course and assignment links in the user's default browser. | Handcrafted `std::process::Command` per operating system. |
| [`rusqlite`](https://crates.io/crates/rusqlite) | `0.40.2` | Local relational cache database | SQLite embedded via `bundled` amalgam. ACID compliance, WAL mode concurrency, indexing for fast lookups, and relational queries for courses/assignments. | `sled` (v1.0.0-alpha.124, perpetual alpha, file format stability risks), plain JSON files (write concurrency races, full-file re-reads). |
| [`thiserror`](https://crates.io/crates/thiserror) | `2.0.20` | Domain error definitions | Strongly-typed error enums for API, Auth, Cache, and Config modules. Zero runtime overhead, clean derive macros, inspectable error variants. | Manual `std::fmt::Display` and `std::error::Error` boilerplate. |
| [`anyhow`](https://crates.io/crates/anyhow) | `1.0.104` | Application-level error handling | Flexible top-level error propagation (`anyhow::Result<()>`) for CLI command handlers with contextual `.context("...")` annotations. | `color-eyre` (v0.6.5, heavy panic-hook overhead), `miette` (v7.6.0, excessive diagnostic complexity for simple CLI workflows). |
| [`wiremock`](https://crates.io/crates/wiremock) | `0.6.5` | HTTP integration test mock server | Runs an embedded HTTP mock server on an ephemeral port. Native async Tokio support, declarative request matchers, clean reset lifecycle. | `httpmock` (v0.8.3, good alternative, but wiremock offers tighter Tokio and reqwest integration). |
| [`assert_cmd`](https://crates.io/crates/assert_cmd) | `2.2.2` | CLI integration testing | Executes compiled binaries in tests, validating exit codes, stdout, stderr, and environment variables. | Manual `std::process::Command` assertions. |
| [`predicates`](https://crates.io/crates/predicates) | `3.1.4` | Composable test assertions | Provides composable string, regex, and file assertions for use with `assert_cmd`. | Manual string substring checks and regex assertions. |
| [`insta`](https://crates.io/crates/insta) | `1.48.0` | Snapshot testing | Snapshot testing for CLI tables, formatted text, and JSON outputs with interactive CLI review (`cargo insta review`). | Fragile hardcoded string literals in integration test files. |
| [`cargo-nextest`](https://crates.io/crates/cargo-nextest) | `0.9.143` | Next-generation test runner | Runs integration and unit tests in parallel processes with execution isolation, clean terminal output, and test retry support. | Standard `cargo test` (serial test bottlenecks, interleaved terminal output). |
| [`ratatui`](https://crates.io/crates/ratatui) | `0.30.2` | Terminal UI framework (optional) | Community-driven standard TUI library. Fast, modular widget framework (`Table`, `List`, `Block`) for an interactive dashboard view. | `cursive` (event-loop heavy), `tui-rs` (deprecated and unmaintained). |
| [`crossterm`](https://crates.io/crates/crossterm) | `0.29.0` | Cross-platform terminal backend | Pure Rust terminal manipulation (raw mode, cursor movement, event polling) for macOS, Linux, and Windows. | `termion` (Unix-only), `ncurses` (C FFI dependency). |
| [`cargo-dist`](https://crates.io/crates/cargo-dist) | `0.32.0` | Release packaging and CI generator | Release automation tool by axodotdev. Generates GitHub Actions workflows for multi-platform compilation, universal binaries, installers, and Homebrew tap PRs. | Manual GitHub Actions shell scripts, `cargo-tarball`. |
| [`cargo-binstall`](https://crates.io/crates/cargo-binstall) | `1.23.0` | Binary package installer | Enables users to install pre-compiled binaries from GitHub Releases without compiling from source. | `cargo install` (requires full Rust toolchain and long compilation times). |
| [`release-plz`](https://crates.io/crates/release-plz) | `0.3.164` | Release automation from PRs | Automated semantic version bumps, changelog generation from conventional commits, and automated release PR creation. | `cargo-release` (v1.1.5, local maintainer CLI execution rather than automated CI PR flow). |
| [`cargo-deny`](https://crates.io/crates/cargo-deny) | `0.20.2` | Dependency linting and security gate | CI linter for cargo dependencies: verifies open-source licenses, flags duplicate dependencies, bans unwanted crates, and checks security advisories. | Manual dependency review. |
| [`cargo-audit`](https://crates.io/crates/cargo-audit) | `0.22.2` | Security advisory scanner | Scans `Cargo.lock` against the RustSec Advisory Database for reported security vulnerabilities. | Manual vulnerability monitoring. |
| [`canvas-lms-api`](https://crates.io/crates/canvas-lms-api) | `1.0.0` | Canvas LMS REST API client | Async Rust client library for the Instructure Canvas LMS REST API. | `canvasapi` (v0.5.0, unmaintained since 2022), `canvas_lms_connector` (v0.1.7, minimal coverage). |

---

## 2. Recommended `[dependencies]` Block

```toml
[package]
name = "canvas-cli"
version = "0.1.0"
edition = "2024"
rust-version = "1.85.0"
authors = ["Canvas CLI Contributors"]
license = "MIT OR Apache-2.0"
description = "Fast, cross-platform CLI for Canvas LMS"
readme = "README.md"

[dependencies]
# CLI Parsing
clap = { version = "4.6.6", features = ["derive", "env", "cargo"] }
clap_complete = "4.6.9"
clap_mangen = "0.3.3"

# Async Runtime & HTTP
tokio = { version = "1.53.1", features = ["rt", "macros", "fs", "io-util", "time"] }
reqwest = { version = "0.13.5", default-features = false, features = [
    "rustls",
    "json",
    "stream",
    "gzip",
    "deflate",
    "brotli",
] }
futures-util = "0.3.34"

# Data Serialization & Date/Time
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
jiff = { version = "0.2.35", features = ["serde"] }

# Secrets Storage
keyring = { version = "4.2.0", default-features = false, features = [
    "apple-native",
    "windows-native",
    "sync-secret-service",
] }

# Configuration & Directories
directories = "6.0.0"
figment = { version = "0.10.19", features = ["toml", "env"] }
toml = "1.1.5"

# Terminal Output, Colors & Progress
comfy-table = { version = "8.0.0", features = ["crossterm", "ansi"] }
anstyle = "1.0.14"
anstream = "1.0.0"
indicatif = "0.18.6"
open = "5.4.3"

# Local Storage & Caching
rusqlite = { version = "0.40.2", features = ["bundled"] }

# Error Handling
thiserror = "2.0.20"
anyhow = { version = "1.0.104", features = ["backtrace"] }

# Optional Interactive TUI Dashboard (feature-gated)
ratatui = { version = "0.30.2", optional = true, default-features = false, features = ["crossterm"] }
crossterm = { version = "0.29.0", optional = true }

[features]
default = []
tui = ["dep:ratatui", "dep:crossterm"]

[dev-dependencies]
wiremock = "0.6.5"
assert_cmd = "2.2.2"
predicates = "3.1.4"
insta = { version = "1.48.0", features = ["json", "yaml"] }
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros"] }

# Project-wide linting configuration
[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
pedantic = { level = "warn", priority = -1 }
nursery = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
must_use_candidate = "allow"
missing_errors_doc = "allow"
```

---

## 3. Detailed Technical Analysis & Notes

### 3.1. Toolchain & Rust Edition
- **Current Stable Rust Version:** **`1.98.1`** (Released September 3, 2026; point release following `1.98.0` on August 20, 2026, fixing a vtable code generation issue).
- **Rust Edition 2024 Status:** Stabilized in **`Rust 1.85.0`** on **February 20, 2025**. In late 2026, Edition 2024 is mature, fully supported across standard CI images and toolchains, and should be adopted for all new codebases. Key benefits include improved borrow-checker ergonomics, refined RPITIT (Return Position Impl Trait In Trait), and updated standard prelude bindings.
- **MSRV Advice:** Set `rust-version = "1.85.0"` in `Cargo.toml`. `1.85.0` is the foundational compiler release supporting `edition = "2024"`. For an end-user binary application (unlike a library), tracking an MSRV of stable `N-2` or `1.85.0` provides sufficient flexibility while ensuring compatibility with common Linux distribution containers.
- **Reference:** [Announcing Rust 1.98.1](https://blog.rust-lang.org/2026/09/03/Rust-1.98.1.html), [Rust 1.85.0 Edition 2024 Announcement](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html).

---

### 3.2. CLI Parsing: Clap Ecosystem
- **`clap` (v4.6.6):** Use the `derive` feature. It provides declarative argument schemas directly on Rust structs and enums. It generates clean `--help` screens, enforces mutual exclusion, and binds environment variables (e.g., `CANVAS_API_TOKEN`).
- **`clap_complete` (v4.6.9):** Shell completion scripts (Bash, Zsh, Fish, PowerShell, Elvish) must be generated in two ways:
  1. At runtime via a dedicated command: `canvas completions <SHELL>`, printing completion scripts to stdout for dynamic evaluation (`source <(canvas completions zsh)`).
  2. Ahead-of-time in packaging scripts (`build.rs` or release workflows) to distribute alongside Debian packages and Homebrew bottles.
- **`clap_mangen` (v0.3.3):** Generates standard Unix roff man pages (`canvas.1`, `canvas-courses.1`) from `clap::Command` definitions. Integrating this into the release pipeline allows automatic man page installation for Homebrew and Linux distribution packages.
- **Rejected Alternatives:**
  - `lexopt`: Requires manual token-by-token parsing; lacks declarative subcommands and automatic help formatting.
  - `argh`: Minimalist, but lacks shell completion generation, man page synthesis, and rich error diagnostics.
  - `bpaf`: Powerful combinator approach, but lacks broad team familiarity compared to Clap's standard derive syntax.

---

### 3.3. HTTP, Async Runtime & Concurrency Control
- **`reqwest` (v0.13.5) vs `ureq` (v3.4.1):**
  - Select **`reqwest`**. Reqwest is built directly on Tokio and `hyper`. In version 0.13, `rustls` uses the `rustls-platform-verifier` backend by default, linking directly to native operating system trust stores (macOS Keychain, Windows Certificate Store, Linux system certs) without compiling OpenSSL or vendoring static Mozilla roots.
  - Streaming file downloads directly pipe `reqwest::Response::bytes_stream()` into a `tokio::fs::File`, preventing multi-megabyte course attachments from buffering into RAM.
  - `ureq` (v3.4.1) introduced async support, but reqwest remains superior for high-throughput streaming, integration with `wiremock`, and simultaneous support for GraphQL and REST JSON bodies.
- **Tokio Runtime Guidance for CLI Downloads:**
  - **Single-Threaded Runtime (`flavor = "current_thread"`):** Recommended for CLI entry points (`#[tokio::main(flavor = "current_thread")]`). Network I/O is asynchronous and non-blocking on a single operating-system thread. This eliminates multi-threaded cross-core synchronization overhead, reduces binary size, and ensures sub-10ms CLI command startup latency.
  - File disk writes (`tokio::fs`) automatically utilize Tokio's internal blocking threadpool without blocking the reactor.
  - Multi-threaded runtime (`flavor = "multi_thread"`) should only be considered if CPU-intensive parallel decompression (e.g. multi-gigabyte zip archives) is required.
- **Concurrency Limiting via `buffer_unordered`:**
  - Canvas LMS imposes strict API rate limits (token leaky-bucket starting at 700 tokens, throttling at 403 Forbidden).
  - Never run unbounded `tokio::spawn` loops for batch downloads.
  - Use `futures_util::stream::StreamExt::buffer_unordered`:
    ```rust
    use futures_util::stream::{self, StreamExt};

    let download_concurrency = 4; // Configurable: 2 to 8
    stream::iter(download_tasks)
        .map(|task| download_file(&client, task))
        .buffer_unordered(download_concurrency)
        .for_each(|result| async {
            // Handle download completion and update indicatif progress bar
        })
        .await;
    ```
  - This keeps exactly `N` downloads in flight concurrently, preventing network congestion, file descriptor exhaustion, and rate-limit penalties.

---

### 3.4. Data: Serialization & Date-Time Selection in 2026
- **`serde` (v1.0.229) & `serde_json` (v1.0.151):** The uncontested standard for deserializing Canvas REST API and GraphQL JSON payloads.
- **Date-Time Crate Choice: `jiff` (v0.2.35) vs `chrono` (v0.4.45) vs `time` (v0.3.55):**
  - **Recommendation: Adopt `jiff`**.
  - `jiff` (created by Andrew Gallant / BurntSushi) is designed around the TC39 Temporal standard.
  - **Why `jiff` wins in 2026:**
    1. **Soundness:** Completely avoids historical undefined behavior and thread-safety bugs associated with libc `localtime_r` in `chrono` and `time`.
    2. **Built-in Time Zone DB:** Seamlessly reads system timezone databases on Linux and macOS, embeds tzdb on Windows, and guarantees safe local timezone lookups without platform-specific initialization boilerplate.
    3. **ISO 8601 Correctness:** Canvas LMS emits RFC 3339 / ISO 8601 strings (e.g., `2026-09-15T23:59:00Z`). Parsing and converting to the student's local system time is a single, panic-free call:
       ```rust
       let zdt = jiff::Timestamp::parse("2026-09-15T23:59:00Z")?
           .to_zoned(jiff::tz::TimeZone::system());
       ```
    4. **Serde Integration:** First-class deserialization support via the `serde` feature flag.
  - `chrono` (v0.4.45) remains ubiquitous as legacy infrastructure, but its complex timezone APIs and API baggage make `jiff` the superior modern choice for a greenfield 2026 CLI.
  - `time` (v0.3.55) has rigid formatting macro constraints and less ergonomic timezone discovery.

---

### 3.5. Secrets: `keyring` Crate & Fallback Architecture
- **`keyring` (v4.2.0):**
  - Interfaces natively with platform security APIs:
    - **macOS:** Apple Keychain via the Security framework.
    - **Windows:** Windows Credential Manager.
    - **Linux:** Secret Service API over D-Bus (`zbus`).
- **Known Pain Points:**
  - **Headless Linux / SSH / Docker / CI:** The Linux Secret Service requires a running D-Bus session bus and an unlocked daemon (e.g., `gnome-keyring-daemon`). In headless environments, calls fail immediately with platform errors or cause long connection timeouts.
  - **CI Runners:** GitHub Actions and containerized workflows have no unlocked user session.
- **Robust Multi-Tier Fallback Resolution Strategy:**
  1. **Tier 1 (Environment Variable):** Check `CANVAS_API_TOKEN` / `CANVAS_TOKEN`. If set, bypass OS keyrings entirely. This is essential for CI, automated scripts, and headless containers.
  2. **Tier 2 (System Keyring):** Attempt `keyring::Entry::new("canvas-cli", "api_token")?.get_password()`.
  3. **Tier 3 (Encrypted / Restricted Config File Fallback):** If `keyring` returns a platform failure or headless error, fall back to reading `~/.config/canvas-cli/credentials.toml` (or `~/.config/canvas-cli/token`).
  4. **Strict Permission Enforcement (`0600`):** When writing credentials to disk, enforce strict Unix permissions readable and writable only by the owner:
     ```rust
     #[cfg(unix)]
     {
         use std::os::unix::fs::PermissionsExt;
         let mut perms = std::fs::metadata(&token_path)?.permissions();
         perms.set_mode(0o600);
         std::fs::set_permissions(&token_path, perms)?;
     }
     ```
     On Windows, restrict ACLs to the user's security identifier (SID).

---

### 3.6. Config and Paths
- **Path Resolution: `directories` (v6.0.0) vs `etcetera` (v0.11.0) vs `dirs` (v7.0.0):**
  - **Recommendation: `directories` (v6.0.0)**.
  - `directories::ProjectDirs::from("com", "instructure", "canvas-cli")` maps to native operating system conventions:
    - **Configuration:**
      - macOS: `~/Library/Application Support/canvas-cli/config.toml` (or `~/.config/canvas-cli/config.toml` if supporting Unix-style override).
      - Linux: `~/.config/canvas-cli/config.toml` (XDG compliance).
      - Windows: `%APPDATA%\canvas-cli\config\config.toml`.
    - **Cache:**
      - macOS: `~/Library/Caches/canvas-cli`.
      - Linux: `~/.cache/canvas-cli`.
      - Windows: `%LOCALAPPDATA%\canvas-cli\cache`.
    - **Data / SQLite DB:**
      - macOS: `~/Library/Application Support/canvas-cli/data.db`.
      - Linux: `~/.local/share/canvas-cli/data.db`.
      - Windows: `%LOCALAPPDATA%\canvas-cli\data\data.db`.
  - `etcetera` (v0.11.0) is a valid alternative if strict XDG layout on macOS is preferred by the CLI userbase.
  - `dirs` (v7.0.0) is low-level and requires manual application directory appending.
- **Hierarchical Configuration: `figment` (v0.10.19) + `toml` (v1.1.5):**
  - `figment` merges configuration sources in explicit precedence order:
    1. Built-in compiled defaults (e.g. default Canvas domain, pagination size 50, download concurrency 4).
    2. User configuration file: `config.toml`.
    3. Environment variables prefixed with `CANVAS_` (e.g. `CANVAS_BASE_URL`).
    4. Explicit CLI argument flags passed on the command line.
  - Emits span-highlighted error messages if the user enters invalid TOML types.
  - `config` (v0.15.25) is rejected due to looser type safety and less descriptive validation error output.

---

### 3.7. Output Formatting, Styling & Terminal Interaction
- **Tables: `comfy-table` (v8.0.0) vs `tabled` (v0.22.0):**
  - **Recommendation: `comfy-table` (v8.0.0)**.
  - Canvas data contains variable-length strings (e.g., assignment names, submission feedback, course titles). `comfy-table` queries the terminal dimensions via crossterm and automatically wraps cell content without clipping or distorting layout. It includes built-in UTF-8 border presets and ANSI styling.
  - `tabled` (v0.22.0) has an excellent `#[derive(Tabled)]` macro, but dynamic terminal wrapping requires extra manual modifier configuration.
- **Styling & Color Negotiation: `anstyle` (v1.0.14) & `anstream` (v1.0.0):**
  - Use `anstyle` for ANSI style definitions and `anstream` for wrapping output streams.
  - `anstream` automatically evaluates terminal capabilities:
    - Automatically strips escape sequences when stdout is redirected to a pipe, file, or `/dev/null`.
    - Automatically honors the industry-standard `NO_COLOR` environment variable (http://no-color.org).
    - Honors `CLICOLOR` and `CLICOLOR_FORCE`.
  - Expose a global CLI flag `--color <WHEN>` (`auto`, `always`, `never`), binding directly to `anstream::ColorChoice` and `clap::ColorChoice`.
  - `owo-colors` (v4.4.0) is rejected to avoid pulling in duplicate color styling engines alongside Clap's internal `anstyle` dependency.
- **Progress Bars: `indicatif` (v0.18.6):**
  - Provides spinning indicators for REST/GraphQL network queries and byte-progress bars for file downloads.
  - Use `indicatif::MultiProgress` to render concurrent file downloads cleanly.
  - Automatically hides progress indicators when stdout is not an interactive terminal.
- **System Launching: `open` (v5.4.3):**
  - Enables commands like `canvas open <assignment-id>` to launch the course page directly in the default browser.
  - Dispatches natively to `open` (macOS), `xdg-open` (Linux), and `start` (Windows).

---

### 3.8. Local Cache Architecture
- **Storage Engine: `rusqlite` (v0.40.2, `bundled`) vs `sled` vs Plain JSON:**
  - **`sled` (v1.0.0-alpha.124):** Rejected. Remained in alpha status for years, with persistent warnings regarding on-disk format instability and data corruption across upgrades.
  - **Plain JSON Files:** Suitable only for static binary blobs. For structured API data, plain files suffer from file-locking race conditions when multiple CLI processes execute concurrently, and lack query capabilities.
  - **Recommendation: `rusqlite` (v0.40.2, `bundled`)**.
    - Bundles the SQLite C amalgam directly into the crate; requires no external SQLite system library installed on macOS, Linux, or Windows.
    - Enable SQLite WAL (Write-Ahead Logging) mode: `PRAGMA journal_mode = WAL;`. This allows concurrent readers without blocking write transactions.
- **Caching Strategy with HTTP Validation (ETags / Timestamps):**
  - Create a lightweight metadata table:
    ```sql
    CREATE TABLE IF NOT EXISTS http_cache (
        endpoint TEXT PRIMARY KEY,
        etag TEXT,
        last_modified TEXT,
        cached_at INTEGER NOT NULL,
        payload BLOB NOT NULL
    );
    CREATE TABLE IF NOT EXISTS courses (
        id INTEGER PRIMARY KEY,
        course_code TEXT,
        name TEXT,
        updated_at TEXT,
        data JSON NOT NULL
    );
    CREATE TABLE IF NOT EXISTS assignments (
        id INTEGER PRIMARY KEY,
        course_id INTEGER NOT NULL,
        due_at TEXT,
        name TEXT,
        data JSON NOT NULL,
        FOREIGN KEY(course_id) REFERENCES courses(id)
    );
    ```
  - **Conditional Request Flow:**
    1. Before dispatching an HTTP GET to Canvas, query `http_cache` for the endpoint.
    2. If found, populate request headers:
       - `If-None-Match: <etag>`
       - `If-Modified-Since: <last_modified>`
    3. If Canvas returns `HTTP 304 Not Modified`, immediately serve the cached SQLite payload without downloading body data. This preserves the user's Canvas rate-limit bucket.
    4. If Canvas returns `HTTP 200 OK`, atomically update the cache table with the new `ETag`, `Last-Modified`, and JSON payload.

---

### 3.9. Error Handling
- **Combo: `thiserror` (v2.0.20) + `anyhow` (v1.0.104):**
  - **`thiserror` for Internal Domain Errors:** Define explicit error enums in internal modules (`src/error.rs`):
    ```rust
    #[derive(thiserror::Error, Debug)]
    pub enum CanvasError {
        #[error("Authentication failed: invalid or expired API token")]
        Unauthorized,
        #[error("Canvas rate limit exceeded; retry after {retry_after} seconds")]
        RateLimited { retry_after: u64 },
        #[error("Network error communicating with Canvas: {0}")]
        Network(#[from] reqwest::Error),
        #[error("Local cache database error: {0}")]
        Database(#[from] rusqlite::Error),
        #[error("Configuration error: {0}")]
        Config(String),
    }
    ```
    This enables program logic to pattern-match on variants (e.g. automatically backing off on `RateLimited` or prompting for login on `Unauthorized`).
  - **`anyhow` at the CLI Binary Boundary:**
    - Use `anyhow::Result<()>` for `main()` and CLI command handlers.
    - Attach contextual diagnostic messages: `.context("Failed to connect to Canvas instance at canvas.instructure.com")`.
    - Handles printing clean error causes to stderr with optional backtraces when `RUST_BACKTRACE=1`.
- **Rejected Alternatives:**
  - `miette` (v7.6.0): Outstanding for compilers, code linters, and DSLs with source code span rendering, but adds unnecessary complexity and visual formatting overhead for a network CLI.
  - `color-eyre` (v0.6.5): Panic-hook focused, heavier runtime overhead.

---

### 3.10. Testing Infrastructure
- **HTTP Mocking: `wiremock` (v0.6.5) vs `httpmock` (v0.8.3):**
  - **Recommendation: `wiremock` (v0.6.5)**.
  - Runs an isolated HTTP mock server bound to `127.0.0.1` on an ephemeral OS port.
  - Designed specifically for async Tokio test suites:
    ```rust
    let mock_server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/api/v1/courses"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(mock_courses))
        .mount(&mock_server)
        .await;
    ```
- **CLI Integration Testing:**
  - **`assert_cmd` (v2.2.2):** Spawns the compiled `canvas` binary as an external subprocess, passing CLI arguments, environment variables, and stdin. Verifies exit status codes.
  - **`predicates` (v3.1.4):** Composable assertions on stdout and stderr (`predicate::str::contains("CS101")`).
  - **`insta` (v1.48.0):** Snapshot testing. Renders table and JSON outputs to `.snap` files. When CLI layouts change, diffs can be reviewed interactively via `cargo insta review`.
- **Test Runner: `cargo-nextest` (v0.9.143):**
  - Next-generation test runner. Executes integration tests in separate, isolated processes in parallel.
  - Drastically outperforms `cargo test`, provides clean progress reporting, and handles flaky network test retries.

---

### 3.11. Optional TUI: Dashboard View
- **Stack: `ratatui` (v0.30.2) + `crossterm` (v0.29.0):**
  - `ratatui` is the leading terminal user interface library in the Rust ecosystem (the active successor to `tui-rs`).
  - `crossterm` provides the cross-platform terminal manipulation backend (raw mode, keyboard input polling, alternate screen buffer).
- **Scope & Architecture:**
  - Implement **strictly as an optional dashboard view** (`canvas dashboard` or `canvas tui`).
  - Feature-gate in `Cargo.toml`:
    `ratatui = { version = "0.30.2", optional = true, default-features = false, features = ["crossterm"] }`
  - Keeps base CLI compilation times fast and binary size minimal for users who do not need an interactive interface.
  - The dashboard presents a read-only terminal overview: active courses list, upcoming assignment deadlines for the current week, and unread announcements, navigable with arrow/Vim keys (`j`/`k`/`Enter` to open in browser).

---

### 3.12. Distribution, Code Signing & Release Engineering in 2026
- **`cargo-dist` (v0.32.0, developed by axodotdev):**
  - The standard release engineering tool for Rust CLIs.
  - Generates comprehensive GitHub Actions CI release workflows (`release.yml`).
  - Automatically builds multi-platform binaries on tag push:
    - `x86_64-apple-darwin` (Intel macOS)
    - `aarch64-apple-darwin` (Apple Silicon macOS)
    - `x86_64-unknown-linux-gnu` / `x86_64-unknown-linux-musl`
    - `aarch64-unknown-linux-gnu` / `aarch64-unknown-linux-musl`
    - `x86_64-pc-windows-msvc`
  - Generates shell installer scripts (`curl --proto '=https' --tlsv1.2 -sSf https://.../install.sh | sh`), PowerShell scripts, and GitHub Release asset archives.
- **Homebrew Tap & `cargo-binstall`:**
  - `cargo-dist` can automatically submit release PRs to a dedicated Homebrew tap (`homebrew-tap`).
  - Supports `cargo-binstall` (v1.23.0), allowing users to install pre-compiled binaries via `cargo binstall canvas-cli` without local compilation.
- **macOS Universal Binary:**
  - macOS binaries should be compiled for both Intel (`x86_64`) and Apple Silicon (`aarch64`) and combined using `lipo`:
    ```bash
    lipo -create -output canvas-universal \
        target/x86_64-apple-darwin/release/canvas \
        target/aarch64-apple-darwin/release/canvas
    ```
  - `cargo-dist` supports universal macOS binary merging out of the box.
- **macOS Code Signing & Notarization Requirements in 2026:**
  - macOS Gatekeeper enforces strict checks on binaries downloaded via web browsers or curl. Unsigned or unnotarized binaries are quarantined (`com.apple.quarantine`) and killed by the OS.
  - **Signing:** Requires an Apple Developer Program subscription ($99/year) and a Developer ID Application certificate:
    ```bash
    codesign --force --options runtime \
        --sign "Developer ID Application: Your Name (TEAMID)" \
        --timestamp canvas
    ```
  - **Notarization:** Binaries must be zipped and uploaded to Apple's notary service using `notarytool`:
    ```bash
    xcrun notarytool submit canvas.zip \
        --apple-id "developer@example.com" \
        --team-id "TEAMID" \
        --password "app-specific-password" \
        --wait
    xcrun stapler staple canvas
    ```
- **Automated Releases: `release-plz` (v0.3.164) vs `cargo-release` (v1.1.5):**
  - **Recommendation: `release-plz` (v0.3.164)**.
  - Continuously monitors the repository `main` branch, analyzes conventional commits (`feat:`, `fix:`), bumps semantic versions, updates `CHANGELOG.md`, and automatically maintains an open Release Pull Request. Merging the PR triggers `cargo-dist` to publish release artifacts.
  - `cargo-release` is an alternative for manual, maintainer-driven local CLI tagging.

---

### 3.13. Code Quality, Lints & Security Auditing
- **Centralized `[lints]` Table (Rust 1.74+ / Edition 2024):**
  Configure compiler and Clippy lints directly in `Cargo.toml`:
  ```toml
  [lints.rust]
  unsafe_code = "forbid"

  [lints.clippy]
  pedantic = { level = "warn", priority = -1 }
  nursery = { level = "warn", priority = -1 }
  module_name_repetitions = "allow"
  must_use_candidate = "allow"
  missing_errors_doc = "allow"
  ```
- **`cargo-deny` (v0.20.2):**
  - Enforce as a mandatory CI pull-request check:
    1. **Licenses:** Whitelists permissive open-source licenses (`MIT`, `Apache-2.0`, `BSD-3-Clause`, `ISC`, `Unicode-3.0`); rejects copyleft licenses (`GPL`, `AGPL`).
    2. **Bans:** Prevents duplicate dependency versions across the crate tree to keep binary size low.
    3. **Advisories:** Automatically queries the RustSec Advisory Database.
    4. **Sources:** Ensures all dependencies originate from `crates.io`.
- **`cargo-audit` (v0.22.2):**
  - Scans `Cargo.lock` on scheduled CI cron jobs against newly published vulnerabilities in the RustSec database.

---

### 3.14. Existing Canvas LMS Rust Crates on crates.io
A search of the [crates.io](https://crates.io) registry identifies several existing crates targeting the Instructure Canvas LMS ecosystem:

1. **[`canvas-lms-api`](https://crates.io/crates/canvas-lms-api) (v1.0.0):**
   - **Author:** Robert Conde (Repository: https://github.com/RobertConde/canvas-lms-api).
   - **Updated:** May 2026.
   - **Description:** A dedicated async Rust client library wrapping the Canvas LMS REST API. Provides typed models for courses, assignments, enrollments, and pagination helpers over `reqwest`.
2. **[`canvas_lms_connector`](https://crates.io/crates/canvas_lms_connector) (v0.1.7):**
   - **Author:** afmiguel (Repository: https://github.com/afmiguel/canvas_lms_connector).
   - **Updated:** September 2024.
   - **Description:** Library providing utility functions for interacting with the Canvas Learning Management System API.
3. **[`canvasapi`](https://crates.io/crates/canvasapi) (v0.5.0):**
   - **Author:** Thomas van der Veldt (Repository: https://gitlab.com/thvdveld/canvasapi).
   - **Updated:** December 2022 (inactive/unmaintained).
   - **Description:** Early experimental Rust wrapper for the Canvas LMS REST API.
4. **[`fuller_canvas_api`](https://crates.io/crates/fuller_canvas_api) (v0.1.6) & [`fuller`](https://crates.io/crates/fuller) (v0.1.6):**
   - **Author:** Grant Lemons (Repository: https://github.com/grantlemons/canvas-cli).
   - **Updated:** April 2024.
   - **Description:** API client and CLI tool used to interact with Instructure Canvas LMS.
5. **[`canvas-cli`](https://crates.io/crates/canvas-cli) (v0.1.0):**
   - **Author:** mbund (Repository: https://github.com/mbund/canvas-cli).
   - **Updated:** March 2025.
   - **Description:** CLI interface for interacting with Canvas LMS from the terminal.
6. **Domain-Specific Utilities:**
   - **`canvas-lms-sync` (v0.3.0):** Synchronizes course files and modules from Canvas LMS to a local folder.
   - **`canvas_syncer` (v0.6.8):** Course file synchronization utility.
   - **`canvas-downloader` (v0.4.2):** Course material download and organization tool.
   - **`canvas-grading` (v0.2.2):** CLI tool for fetching student submissions and uploading grades.

---

## 4. Verification Attestation
All crate versions and tool availability cited in this report were verified directly against the live [crates.io](https://crates.io) API (`https://crates.io/api/v1/crates/{crate}`) and official Rust project releases on **September 9, 2026**. No items are unverified.
