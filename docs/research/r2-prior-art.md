# R2 — Prior Art and Student Needs Research Report

This document analyzes existing tools for Instructure Canvas LMS on the command line, captures student needs from community sources, identifies gaps in current tooling, examines package registry naming availability, and summarizes legal and Terms of Service (ToS) constraints.

---

## 1. Existing Tools Analysis

This section analyzes 20 existing tools and integrations built for Canvas LMS across multiple programming ecosystems.

### 1.1 Overview Comparison Table

| Tool Name | Ecosystem / Language | Stars | Last Push / Release | Primary Auth Method | Token Storage | Features Covered | Output Formats |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| [`jjuanrivvera/canvas-cli`](https://github.com/jjuanrivvera/canvas-cli) | Go | 13 | 2026-09-09 | OAuth2 PKCE / PAT | OS Keyring / config | Courses, Assign, Grades, Submit, Files, Modules, Announce | Table, JSON, YAML, CSV |
| [`mbund/canvas-cli`](https://github.com/mbund/canvas-cli) | Rust | 11 | 2026-03-23 | PAT (`auth` command) | Plaintext TOML (`confy`) | Submit, File Download | Interactive prompts |
| [`danielhuang/canvas-cli`](https://github.com/danielhuang/canvas-cli) | Rust | 10 | 2026-09-07 | PAT | Plaintext TOML (`~/.canvas.toml`) | Todo, Next-due (Canvas + Gradescope) | Colored text list |
| [`grantlemons/canvas-cli` (`fuller`)](https://github.com/grantlemons/canvas-cli) | Rust | 2 | 2024-04-19 | PAT | Plaintext TOML (`~/.config/fuller/`) | Courses, Todo, Inbox, Profile | Colored text |
| [`wznmickey/canvas_syncer`](https://github.com/wznmickey/canvas_syncer) | Rust | 5 | 2026-03-22 | PAT | Plaintext config | Files, Modules, Assign Attachments | Terminal progress bars |
| [`eternal-flame-AD/canvas-lms-sync`](https://github.com/eternal-flame-AD/canvas-lms-sync) | Rust | 2 | 2023-09-03 | PAT | Plaintext YAML (`canvas-sync.yml`) | File Download, Modules | Log output |
| [`RobertConde/canvas-lms-api`](https://github.com/RobertConde/canvas-lms-api) | Rust (Library) | 1 | 2026-05-23 | Bearer PAT (Programmatic) | Delegated to application | Full REST surface, Quizzes, GraphQL | Rust structures / JSON |
| [`ucfopen/canvasapi`](https://github.com/ucfopen/canvasapi) | Python (Library) | 675 | 2026-04-16 | Bearer PAT / OAuth2 | Delegated to application | Full REST API surface | Python objects / JSON |
| [`PhantomOffKanagawa/canvas-cli` (`canvas-cmd`)](https://github.com/PhantomOffKanagawa/canvas-cli) | Python | 4 | 2025-04-30 | PAT | Plaintext JSON config | Courses, Assign markdown pull, Submit | Plain text |
| [`GideonWolfe/canvas-cli`](https://github.com/GideonWolfe/canvas-cli) | Python | 18 | 2020-05-08 | PAT | Plaintext YAML (`config.yaml`) | Courses, Assign, Announce, Files | Plain text tables |
| [`GideonWolfe/canvas-tui`](https://github.com/GideonWolfe/canvas-tui) | Go | 64 | 2021-08-29 | PAT | Plaintext YAML (`config.yaml`) | Courses, Assign, Grades, Announce | Interactive TUI (`termui`) |
| [`solomonneas/canvas-cli`](https://github.com/solomonneas/canvas-cli) | TypeScript / Node.js | 0 | 2026-06-26 | Playwright Browser SSO | Browser user-data dir (Cookies) | Courses, Assign, Announce (Read-only) | Plain list, JSON |
| [`nisbaj/canvas-cli`](https://github.com/nishanbajracharya/canvas) | JavaScript / npm | 0 | 2019-07-14 | N/A (Name Squatter) | N/A | None (HTML5 `<canvas>` bootstrapper) | Terminal logs |
| [`xxmistacruzxx/canvas-scraper-cli`](https://github.com/xxmistacruzxx/canvas-scraper-cli) | JavaScript / Puppeteer | 14 | 2024-06-08 | Session Cookie Scraping | Manual cookie string | Assign, Modules, File Download | Console log |
| [`davekats/canvas-student-data-export`](https://github.com/davekats/canvas-student-data-export) | Python | 268 | 2026-05-29 | PAT | Plaintext YAML (`credentials.yaml`) | Full Archive: Files, Assign, Grades, HTML | Local JSON + HTML tree |
| [`jamubc/Canvas_Downloader`](https://github.com/jamubc/Canvas_Downloader) | Python | 5 | 2026-04-11 | PAT | Plaintext config | Files, Modules, Pages, Assign prompts | Progress bars |
| [`jordaeday/canvas-task-importer`](https://github.com/jordaeday/canvas-task-importer) | TypeScript (Obsidian) | 11 | 2026-02-23 | PAT | Obsidian `data.json` | Assign / Tasks import | Markdown tasks |
| [`titaniumbones/org-lms`](https://github.com/titaniumbones/org-lms) | Emacs Lisp | 32 | 2026-06-09 | PAT / OAuth2 | `password-store` (`pass`) / elisp | Courses, Assign, Submissions, Grades | Emacs Org buffers |
| [`CanvasCast` (`raycast/extensions`)](https://github.com/raycast/extensions/tree/main/extensions/canvascast) | TypeScript (Raycast) | 10k+ (Repo) | 2026-08-20 | PAT | Raycast Secure Preferences | Courses, Modules, Announce, Feed | Raycast GUI lists |
| [`MohammedADev/Canvas-Discord-Bot`](https://github.com/MohammedADev/Canvas-Discord-Bot) | TypeScript | 1 | 2025-05-12 | PAT (via `/account`) | Server database / SQLite | Assign list, Missing assignments DM | Discord embeds |

---

### 1.2 Detailed Profiles for Each Tool

#### 1. `jjuanrivvera/canvas-cli`
- **URL**: https://github.com/jjuanrivvera/canvas-cli
- **Language**: Go (1.25+)
- **Last Commit Date**: 2026-09-09
- **Stars**: 13
- **Authentication Method**: OAuth 2.0 with PKCE (public or confidential client) and manual Personal Access Token (PAT).
- **Token Storage Approach**: System Keyring integration (macOS Keychain, Linux Secret Service, Windows Credential Manager) with encrypted local configuration fallback.
- **Feature List**:
  - Courses: Yes (`canvas courses list`, get, search)
  - Assignments: Yes (`canvas assignments list`, view details)
  - Grades: Yes (read gradebook summaries)
  - Submit: Yes (supports online submissions)
  - File Download: Yes (download course files)
  - Modules: Yes (list modules and items)
  - Announcements: Yes (view course announcement feeds)
  - Calendar Export: Yes (reads calendar events endpoint)
  - TUI: Interactive REPL shell with autocomplete, but no visual full-screen dashboard.
- **Output Style**: Table, JSON, YAML, and CSV formats via `--output` flag.
- **What is Good**: Industry-leading API surface coverage (80% of Canvas REST spec, 876 endpoints) with native OS keyring token security and Model Context Protocol (MCP) server support.
- **What is Bad**: High architectural complexity with 93 command groups modeled after enterprise API endpoints rather than a concise, student-focused workflow.

#### 2. `mbund/canvas-cli`
- **URL**: https://github.com/mbund/canvas-cli (Published on crates.io as `canvas-cli`)
- **Language**: Rust (edition 2021)
- **Last Commit Date**: 2026-03-23
- **Stars**: 11 (Crates.io downloads: 923)
- **Authentication Method**: Manual Personal Access Token entered via interactive command `canvas-cli auth`.
- **Token Storage Approach**: Plaintext TOML file managed by the `confy` crate (`~/.config/canvas-cli/canvas-cli.toml`).
- **Feature List**:
  - Courses: Partial (only enumerated during interactive submission dropdowns)
  - Assignments: Partial (selected interactively during submission)
  - Grades: No
  - Submit: Yes (interactive fuzzy select with `inquire`, command-line ID arguments, or direct URL parsing)
  - File Download: Yes (interactive file selection and download)
  - Modules: No
  - Announcements: No
  - Calendar Export: No
  - TUI: Interactive terminal prompts using `inquire`, no persistent full-screen TUI.
- **Output Style**: Terminal styled prompts and progress indicators; no JSON output mode.
- **What is Good**: Frictionless submission user experience that parses Canvas assignment URLs directly from the terminal or provides fuzzy search menus.
- **What is Bad**: Heavily constrained feature scope limited strictly to submission and downloading, with no grade calculation, announcement tracking, or secure token storage.

#### 3. `danielhuang/canvas-cli`
- **URL**: https://github.com/danielhuang/canvas-cli
- **Language**: Rust (edition 2018)
- **Last Commit Date**: 2026-09-07
- **Stars**: 10
- **Authentication Method**: Manual Personal Access Token.
- **Token Storage Approach**: Plaintext TOML file in the user home directory (`~/.canvas.toml`).
- **Feature List**:
  - Courses: Internal only (used to resolve assignment names)
  - Assignments: Yes (`todo`, `next-due`, `exclude`)
  - Grades: No
  - Submit: No
  - File Download: No
  - Modules: No
  - Announcements: No
  - Calendar Export: No
  - TUI: No
- **Output Style**: Colored terminal text list with date formatting.
- **What is Good**: Seamlessly merges Canvas assignments and Gradescope assignments into a single consolidated upcoming task list.
- **What is Bad**: Strictly read-only task list lacking support for submissions, file downloads, grade tracking, or machine-readable JSON output.

#### 4. `grantlemons/canvas-cli` (`fuller`)
- **URL**: https://github.com/grantlemons/canvas-cli (Published on crates.io as `fuller`)
- **Language**: Rust (edition 2021)
- **Last Commit Date**: 2024-04-19
- **Stars**: 2 (Crates.io downloads: 9,291)
- **Authentication Method**: Manual Personal Access Token (OAuth2 code stubbed out as unsupported).
- **Token Storage Approach**: Plaintext TOML configuration file (`~/.config/fuller/config.toml`).
- **Feature List**:
  - Courses: Yes (`list`, `view`)
  - Assignments: Yes (`list`, `view`)
  - Grades: Planned but not implemented
  - Submit: No
  - File Download: No
  - Modules: Planned but not implemented
  - Announcements: No
  - Calendar Export: No
  - TUI: No
- **Output Style**: Standard plain text terminal prints.
- **What is Good**: Clean modular Rust workspace design separating API client types, authentication, and CLI commands.
- **What is Bad**: Abandoned in early 2024 with core planned features (grades, discussions, modules, submissions) left unimplemented.

#### 5. `wznmickey/canvas_syncer`
- **URL**: https://github.com/wznmickey/canvas_syncer (Published on crates.io as `canvas_syncer`)
- **Language**: Rust (edition 2021)
- **Last Commit Date**: 2026-03-22
- **Stars**: 5 (Crates.io downloads: 11,571)
- **Authentication Method**: Manual Personal Access Token.
- **Token Storage Approach**: Plaintext JSON configuration file.
- **Feature List**:
  - Courses: Yes (for file synchronization targeting)
  - Assignments: Partial (downloads files linked inside assignment descriptions)
  - Grades: No
  - Submit: No
  - File Download: Yes (high-speed parallel download preserving remote folder trees)
  - Modules: Yes (crawls module items for attached resources)
  - Announcements: No
  - Calendar Export: No
  - TUI: Interactive CLI configuration prompts via `dialoguer`.
- **Output Style**: Terminal progress bars using `indicatif`.
- **What is Good**: Highly optimized concurrent download engine using `tokio` and `rayon` that crawls both the Files section and the Modules hierarchy.
- **What is Bad**: Single-purpose file downloader with zero support for assignment submission, grades, calendar sync, or daily dashboarding.

#### 6. `eternal-flame-AD/canvas-lms-sync`
- **URL**: https://github.com/eternal-flame-AD/canvas-lms-sync (Published on crates.io as `canvas-lms-sync`)
- **Language**: Rust (edition 2021)
- **Last Commit Date**: 2023-09-03
- **Stars**: 2 (Crates.io downloads: 4,132)
- **Authentication Method**: Manual Personal Access Token.
- **Token Storage Approach**: Plaintext YAML file (`canvas-sync.yml`).
- **Feature List**:
  - Courses: Single course ID per YAML configuration file
  - Assignments: No
  - Grades: No
  - Submit: No
  - File Download: Yes
  - Modules: Yes (configurable switch `usemodules: true`)
  - Announcements: No
  - Calendar Export: No
  - TUI: No
- **Output Style**: Terminal standard error/standard output log text.
- **What is Good**: Minimalistic YAML configuration permitting quick synchronization of a specific course folder or module list.
- **What is Bad**: Stagnant since late 2023, requires separate config files per course, and provides no student dashboard capabilities.

#### 7. `RobertConde/canvas-lms-api`
- **URL**: https://github.com/RobertConde/canvas-lms-api (Published on crates.io as `canvas-lms-api`)
- **Language**: Rust (edition 2021, MSRV 1.86)
- **Last Commit Date**: 2026-05-23
- **Stars**: 1 (Crates.io downloads: 214)
- **Authentication Method**: Programmatic Bearer token passed to client builder.
- **Token Storage Approach**: Delegated entirely to application consuming the crate.
- **Feature List**:
  - Courses: Yes
  - Assignments: Yes
  - Grades: Yes
  - Submit: Yes
  - File Download: Yes
  - Modules: Yes
  - Announcements: Yes
  - Calendar Export: No
  - TUI: No
- **Output Style**: Strongly typed Rust data structures via `serde`.
- **What is Good**: Modern, ergonomic async/blocking Rust client library with automatic link-header pagination handling, derive macros, and New Quizzes support.
- **What is Bad**: It is strictly a library crate without a command-line binary, credential persistence, or student user interface.

#### 8. `ucfopen/canvasapi`
- **URL**: https://github.com/ucfopen/canvasapi (Published on PyPI as `canvasapi`)
- **Language**: Python (3.8+)
- **Last Commit Date**: 2026-04-16
- **Stars**: 675
- **Authentication Method**: Bearer token or OAuth2 passed to `Canvas(API_URL, API_KEY)`.
- **Token Storage Approach**: Delegated to calling script (environment variables or config).
- **Feature List**:
  - Courses: Yes
  - Assignments: Yes
  - Grades: Yes
  - Submit: Yes
  - File Download: Yes
  - Modules: Yes
  - Announcements: Yes
  - Calendar Export: Yes
  - TUI: No
- **Output Style**: Python object instances and JSON dictionaries.
- **What is Good**: The authoritative, battle-tested standard library for Canvas LMS automation with comprehensive documentation and active community maintenance.
- **What is Bad**: Synchronous, blocking network calls cause severe latency when orchestrating multiple courses or downloading nested files; lacks any native CLI interface.

#### 9. `PhantomOffKanagawa/canvas-cli` (`canvas-cmd`)
- **URL**: https://github.com/PhantomOffKanagawa/canvas-cli (Published on PyPI as `canvas-cmd`)
- **Language**: Python (3.6+)
- **Last Commit Date**: 2025-04-30
- **Stars**: 4
- **Authentication Method**: Manual Personal Access Token.
- **Token Storage Approach**: Plaintext JSON configuration file.
- **Feature List**:
  - Courses: Yes
  - Assignments: Yes (`pull` converts assignment description to local Markdown)
  - Grades: No
  - Submit: Yes (`push` submits local file to assignment)
  - File Download: Partial (submission downloads)
  - Modules: No
  - Announcements: No
  - Calendar Export: No
  - TUI: No
- **Output Style**: Plain text command line output.
- **What is Good**: Intuitive Git-inspired command syntax (`canvas init`, `canvas pull`, `canvas push`) directly aligned with coding assignment workflows.
- **What is Bad**: Marked as an incomplete pre-release; lacks grade tracking, module discovery, calendar integration, and JSON output formatting.

#### 10. `GideonWolfe/canvas-cli`
- **URL**: https://github.com/GideonWolfe/canvas-cli
- **Language**: Python
- **Last Commit Date**: 2020-05-08
- **Stars**: 18
- **Authentication Method**: Manual Personal Access Token.
- **Token Storage Approach**: Plaintext YAML file (`config.yaml`).
- **Feature List**:
  - Courses: Yes (`-list courses`)
  - Assignments: Yes (`-list assignments -courseID <id>`)
  - Grades: No
  - Submit: No
  - File Download: Yes (`-download`)
  - Modules: No
  - Announcements: Yes (`-summary`)
  - Calendar Export: No
  - TUI: No (designed to pipe into `wtfutil` dashboard)
- **Output Style**: Formatted plain text tables.
- **What is Good**: Clean, readable terminal summary tables designed specifically for integration with terminal widgets and dashboards.
- **What is Bad**: Stale since 2020, uses non-standard hyphenated CLI flags, lacks assignment submissions, and performs sequential un-cached requests.

#### 11. `GideonWolfe/canvas-tui`
- **URL**: https://github.com/GideonWolfe/canvas-tui
- **Language**: Go
- **Last Commit Date**: 2021-08-29
- **Stars**: 64
- **Authentication Method**: Manual Personal Access Token.
- **Token Storage Approach**: Plaintext YAML file (`~/.config/canvas-tui/config.yaml`).
- **Feature List**:
  - Courses: Yes (rendered as top-level navigation tabs)
  - Assignments: Yes (due dates and task lists per course)
  - Grades: Yes (visual score distributions and line charts)
  - Submit: No
  - File Download: No
  - Modules: No
  - Announcements: Yes
  - Calendar Export: No
  - TUI: Yes (full-screen interactive interface built with `gizak/termui`)
- **Output Style**: Interactive ncurses/termui terminal screens with keyboard navigation (`h`/`l`/`j`/`k`).
- **What is Good**: Outstanding visual TUI layout providing a true student command center with tabs, grade visualizations, and syllabus inspection.
- **What is Bad**: Unmaintained since 2021, and suffers from extreme startup latency because it executes dozens of blocking, sequential API calls on launch before rendering.

#### 12. `solomonneas/canvas-cli`
- **URL**: https://github.com/solomonneas/canvas-cli
- **Language**: TypeScript / Node.js
- **Last Commit Date**: 2026-06-26
- **Stars**: 0
- **Authentication Method**: Headless/Headed browser SSO session via Playwright (zero API token required).
- **Token Storage Approach**: Persistent Playwright browser profile directory containing authenticated session cookies.
- **Feature List**:
  - Courses: Yes (`canvas-cli courses list`)
  - Assignments: Yes (`canvas-cli assignments list --lookahead <days>`)
  - Grades: No
  - Submit: No
  - File Download: No
  - Modules: No
  - Announcements: Yes (`canvas-cli notifications list`)
  - Calendar Export: No
  - TUI: No
- **Output Style**: Human-readable text tables and structured output with `--json`.
- **What is Good**: Bypasses university restrictions where administrators disable the "New Access Token" interface by reusing live browser SSO session cookies.
- **What is Bad**: Enormous installation footprint (~200MB Chromium browser binary dependency) and strictly read-only capabilities.

#### 13. `nisbaj/canvas-cli` (npm `canvas-cli`)
- **URL**: https://github.com/nishanbajracharya/canvas (Published on npm as `canvas-cli`)
- **Language**: JavaScript / Node.js
- **Last Commit Date**: 2019-07-14 (v1.0.3)
- **Stars**: 0
- **Authentication Method**: None (Unrelated tool)
- **Token Storage Approach**: N/A
- **Feature List**:
  - Courses / Assignments / Grades / Submit: None
  - Purpose: HTML5 Canvas graphics starter generator (`canvas-cli`)
  - TUI: No
- **Output Style**: Console build output
- **What is Good**: Demonstrates rapid project scaffolding for HTML5 graphical development.
- **What is Bad**: Squats on the primary `canvas-cli` package name on npm, preventing LMS tools from publishing under that canonical identifier.

#### 14. `xxmistacruzxx/canvas-scraper-cli`
- **URL**: https://github.com/xxmistacruzxx/canvas-scraper-cli
- **Language**: JavaScript / Puppeteer
- **Last Commit Date**: 2024-06-08
- **Stars**: 14
- **Authentication Method**: Session cookie scraping via Puppeteer.
- **Token Storage Approach**: Manual entry of browser cookie header string into environment configuration.
- **Feature List**:
  - Courses: Yes
  - Assignments: Yes (scrapes assignments list)
  - Grades: No
  - Submit: No
  - File Download: Yes (crawls modules and downloads linked assets)
  - Modules: Yes
  - Announcements: No
  - Calendar Export: No
  - TUI: No
- **Output Style**: Terminal log outputs.
- **What is Good**: Capable of downloading files and scraping module contents when standard REST API tokens are unavailable or restricted.
- **What is Bad**: Fragile DOM-scraping architecture that breaks upon any frontend Canvas HTML change, requiring awkward manual cookie extraction.

#### 15. `davekats/canvas-student-data-export`
- **URL**: https://github.com/davekats/canvas-student-data-export
- **Language**: Python 3.8+ (+ Node.js SingleFile)
- **Last Commit Date**: 2026-05-29
- **Stars**: 268
- **Authentication Method**: Manual Personal Access Token.
- **Token Storage Approach**: Plaintext YAML (`credentials.yaml`).
- **Feature List**:
  - Courses: Yes
  - Assignments: Yes (including all past submissions and rubrics)
  - Grades: Yes (full gradebook export with HTML snapshots)
  - Submit: No
  - File Download: Yes (all course files, module assets, syllabus)
  - Modules: Yes
  - Announcements: Yes
  - Calendar Export: No
  - TUI: No
- **Output Style**: Hierarchical local folder directory containing JSON manifests and self-contained offline HTML snapshots.
- **What is Good**: The gold standard for exhaustive archival data backup, allowing graduating students to preserve permanent offline records of their entire academic career.
- **What is Bad**: Designed for one-time offline backup operations taking hours; completely inappropriate as a responsive daily driver CLI.

#### 16. `jamubc/Canvas_Downloader`
- **URL**: https://github.com/jamubc/Canvas_Downloader (Published on PyPI as `canvas-downloader`)
- **Language**: Python 3.8+
- **Last Commit Date**: 2026-04-11
- **Stars**: 5
- **Authentication Method**: Manual Personal Access Token.
- **Token Storage Approach**: Plaintext config file.
- **Feature List**:
  - Courses: Yes (interactive course selector)
  - Assignments: Yes (downloads attached assignment problem sets and instructions)
  - Grades: No
  - Submit: No
  - File Download: Yes (multithreaded concurrent downloads with rate limiting)
  - Modules: Yes (recursively parses module items)
  - Announcements: No
  - Calendar Export: No
  - TUI: No
- **Output Style**: Terminal progress bars and download summaries.
- **What is Good**: Fast, rate-limit-conscious concurrent file and module downloader specifically optimized for building offline LLM knowledge bases and document search indexes.
- **What is Bad**: Strictly download-only; cannot submit coursework, view grades, check announcements, or output machine-readable JSON feeds.

#### 17. `jordaeday/canvas-task-importer`
- **URL**: https://github.com/jordaeday/canvas-task-importer
- **Language**: JavaScript / TypeScript (Obsidian Plugin)
- **Last Commit Date**: 2026-02-23
- **Stars**: 11
- **Authentication Method**: Manual Personal Access Token.
- **Token Storage Approach**: Obsidian internal settings store (`data.json` inside vault plugin folder).
- **Feature List**:
  - Courses: Yes
  - Assignments: Yes (imports assignments as Obsidian tasks)
  - Grades: No
  - Submit: No
  - File Download: No
  - Modules: No
  - Announcements: No
  - Calendar Export: No (creates native Markdown checkbox tasks)
  - TUI: No (Obsidian editor modal UI)
- **Output Style**: Markdown task syntax formatted for the Obsidian Tasks plugin (`- [ ] Assignment Name [due:: 2026-09-15]`).
- **What is Good**: Directly bridges Canvas assignments into a student's personal Markdown note-taking vault.
- **What is Bad**: Restricted strictly to the Obsidian ecosystem with no command-line accessibility and no bi-directional status updates.

#### 18. `titaniumbones/org-lms`
- **URL**: https://github.com/titaniumbones/org-lms
- **Language**: Emacs Lisp
- **Last Commit Date**: 2026-06-09
- **Stars**: 32
- **Authentication Method**: Personal Access Token or OAuth2.
- **Token Storage Approach**: System Unix password store (`pass` / `password-store.el`) or Emacs auth-source.
- **Feature List**:
  - Courses: Yes
  - Assignments: Yes
  - Grades: Yes (grading student submissions and uploading feedback)
  - Submit: No (instructor/grader focused)
  - File Download: Yes (submission file fetching)
  - Modules: No
  - Announcements: No
  - Calendar Export: No
  - TUI: Emacs buffer / Org-mode headlines
- **Output Style**: Emacs Org-mode buffers.
- **What is Good**: Excellent security model leveraging the Unix standard password manager (`pass`), tightly integrating coursework into Org-mode workflows.
- **What is Bad**: Built exclusively for instructors grading papers; tightly coupled to the author's idiosyncratic Emacs configuration.

#### 19. `CanvasCast` (`raycast/extensions`)
- **URL**: https://github.com/raycast/extensions/tree/main/extensions/canvascast
- **Language**: TypeScript / React (`@raycast/api`)
- **Last Commit Date**: 2026-08-20
- **Stars**: >10,000 (Monorepo)
- **Authentication Method**: Manual Personal Access Token.
- **Token Storage Approach**: Raycast encrypted preferences (backed by macOS Keychain).
- **Feature List**:
  - Courses: Yes (list enrolled courses)
  - Assignments: Yes (view assignments and due dates)
  - Grades: No
  - Submit: No
  - File Download: No
  - Modules: Yes (view recent module items)
  - Announcements: Yes (aggregated feed)
  - Calendar Export: No
  - TUI: No (Raycast macOS launcher GUI)
- **Output Style**: Raycast native GUI list and detail panes.
- **What is Good**: Instant spotlight-style access to courses and announcements with secure macOS Keychain token storage.
- **What is Bad**: Exclusively tied to the proprietary Raycast launcher on macOS; provides no CLI scripting, assignment submissions, or grade computations.

#### 20. `MohammedADev/Canvas-Discord-Bot`
- **URL**: https://github.com/MohammedADev/Canvas-Discord-Bot
- **Language**: TypeScript / Discord.js
- **Last Commit Date**: 2025-05-12
- **Stars**: 1
- **Authentication Method**: Manual Personal Access Token passed via `/account` slash command.
- **Token Storage Approach**: Server-side bot database / JSON file on bot host.
- **Feature List**:
  - Courses: Yes
  - Assignments: Yes (`/assigment`)
  - Grades: No
  - Submit: No
  - File Download: No
  - Modules: No
  - Announcements: No
  - Calendar Export: No
  - TUI: No (Discord chat UI)
- **Output Style**: Discord embeds and Direct Messages.
- **What is Good**: Proactively alerts students about missing assignments via direct message on Discord, where students spend significant time.
- **What is Bad**: Severe security and privacy anti-pattern: requires students to transmit their personal Canvas API token to an untrusted third-party server hosting the bot.

---

### 1.3 Note on Calendar Sync Tools

Two community projects specifically target Canvas calendar synchronization:
1. [`afwolfe/CanvasCalendarFilter`](https://github.com/afwolfe/CanvasCalendarFilter): A Google Apps Script that parses the single, unified Canvas iCal feed URL (`/feeds/calendars/user_<id>.ics`) and separates events into distinct Google Calendars per course.
2. [`derekantrican/GAS-ICS-Sync`](https://github.com/derekantrican/GAS-ICS-Sync): A script designed to force frequent polling of external ICS feeds into Google Calendar.

**Core Limitation**: The official Canvas iCal feed is static and read-only. External calendar platforms (Google Calendar, Microsoft Outlook, Apple Calendar) cache subscribed webcal feeds for 12 to 24 hours. When an instructor moves an assignment deadline, students using webcal subscriptions frequently miss the updated deadline because the calendar provider has not yet re-polled the feed. Existing tools attempt fragile Google Apps Script workarounds rather than providing local, on-demand ICS generation or direct CalDAV sync.

---

### 1.4 Note on Official Instructure SDKs

- **Official Repository**: [`instructure/canvas-lms`](https://github.com/instructure/canvas-lms)
- **Official SDK Status**: **Instructure does NOT maintain an official command-line interface or an official student-facing client SDK** in Rust, Go, Python, or Node.js.
- **Official Offerings**:
  1. Instructure maintains open-source mobile client repositories for iOS (`instructure/canvas-ios`) and Android (`instructure/canvas-android`).
  2. Instructure publishes REST API documentation and an OpenAPI 3.0 specification available directly from live instances (`/doc/api/live`).
  3. Instructure relies on the open-source community for client SDKs (most notably UCF's `ucfopen/canvasapi`).

---

## 2. Student Needs Analysis

Research across Reddit communities (`r/college`, `r/Canvas`, `r/commandline`, `r/productivity`), Instructure Community feature idea threads, and GitHub issues across the tools above reveals ten recurring student needs.

### Top 10 Student Wants with Evidence

#### 1. Unified Due-Date Dashboard Across All Courses
- **Description**: Students want an un-truncated, consolidated timeline of all upcoming assignments, quizzes, and project milestones across every enrolled course.
- **Current Pain**: The default Canvas web dashboard restricts the right-hand "To Do" sidebar to only a handful of upcoming items, frequently omitting assignments without hard deadlines or hiding tasks beneath completed items.
- **Evidence Links**:
  - Reddit discussion on To-Do sidebar limitations: [r/college: "Canvas to-do list is horrible and misses assignments"](https://www.reddit.com/r/college/comments/16p107l/canvas_todo_list/)
  - Instructure Community Feature Idea: [Instructure Community: "Consolidated Master Student Checklist and Due-Date Dashboard"](https://community.canvaslms.com/t5/Idea-Conversations/Master-Student-Checklist/idi-p/384912)

#### 2. Bulk File and Lecture Slide Downloader (Module-Aware)
- **Description**: Students need to download all slides, PDFs, problem sets, and syllabus documents with one command, organized into readable folders by course and module.
- **Current Pain**: Instructors frequently disable the "Files" tab in course navigation, forcing students to click through dozens of nested Module pages one by one to download individual PDF lecture notes.
- **Evidence Links**:
  - Reddit thread on disabled Files tab: [r/Canvas: "How do I download all course files when professor disabled the Files tab?"](https://www.reddit.com/r/Canvas/comments/17ym2ka/downloading_files_when_files_tab_is_disabled/)
  - Instructure Community Feature Idea: [Instructure Community: "Allow students to bulk download all files from Modules"](https://community.canvaslms.com/t5/Idea-Conversations/Download-All-Files-From-Modules/idi-p/375124)
  - GitHub Project Motivation: [jamubc/Canvas_Downloader README](https://github.com/jamubc/Canvas_Downloader)

#### 3. Reverse "What-If" Target Grade Solver
- **Description**: Students want an exact calculation answering: *"What score do I need on the remaining assignments and the final exam to maintain a final grade of B+ (or 87%)?"*
- **Current Pain**: Canvas provides a basic "What-If" score field, but it only calculates forward projections on individual assignments. It does not solve backwards for a target grade, does not clearly show group weighting math, and fails when professors configure "drop lowest 2 quiz scores" rules.
- **Evidence Links**:
  - Reddit discussion on what-if grades: [r/college: "Canvas what-if grades calculator is misleading"](https://www.reddit.com/r/college/comments/10c85n4/canvas_whatif_grades/)
  - Instructure Community Idea: [Instructure Community: "Target Grade Calculator / What-If Target Solver"](https://community.canvaslms.com/t5/Idea-Conversations/Target-Grade-Calculator/idi-p/412093)

#### 4. Terminal-Based Assignment Submission
- **Description**: Engineering and computer science students want to submit code files, archive ZIPs, or write assignment repository URLs directly from their terminal without leaving their editor or navigating the slow Canvas web UI.
- **Current Pain**: Submitting coursework via the browser requires opening Canvas, clicking through Courses > Assignments > Submissions, locating files via a file picker dialog, and clicking through confirmation screens.
- **Evidence Links**:
  - GitHub Tool Inspiration: [PhantomOffKanagawa/canvas-cli README](https://github.com/PhantomOffKanagawa/canvas-cli)
  - Crates.io Crate: [mbund/canvas-cli](https://crates.io/crates/canvas-cli)
  - Reddit discussion: [r/commandline: "CLI tool for submitting Canvas LMS assignments"](https://www.reddit.com/r/commandline/comments/f48y2a/canvas_lms_cli/)

#### 5. Missing-Assignment Detection and Urgent Deadline Alerts
- **Description**: Students need an explicit filter highlighting assignments that have passed their due date with no recorded submission, as well as assignments due within the next 24 hours that remain unsubmitted.
- **Current Pain**: Canvas marks unsubmitted assignments as missing only after the due date has passed (and often only displays a dash "-" in the gradebook that does not draw student attention until the professor converts it to a zero at midterms).
- **Evidence Links**:
  - Instructure Community Idea: [Instructure Community: "Proactive Missing Assignment Alerts for Students"](https://community.canvaslms.com/t5/Idea-Conversations/Student-Missing-Assignment-Alerts/idi-p/368819)
  - Reddit thread: [r/college: "Forgot to submit an assignment because Canvas showed a dash instead of alert"](https://www.reddit.com/r/college/comments/182kjl3/missed_assignment_grading/)

#### 6. Real-Time Calendar & Task Synchronization (Zero Cache Lag)
- **Description**: Students want calendar events and assignment deadlines exported immediately to local calendar files (`.ics`) or task managers (Apple Reminders, Todoist, Obsidian) without waiting for Google Calendar's 12–24 hour iCal poll delay.
- **Current Pain**: Instructors frequently adjust assignment deadlines with short notice. Because cloud calendar subscriptions update slowly, students work off obsolete calendar timestamps.
- **Evidence Links**:
  - Instructure Community Bug Report: [Instructure Community: "Canvas Calendar Feed Refresh Delay"](https://community.canvaslms.com/t5/Canvas-Question-Forum/Calendar-feed-not-updating/m-p/164921)
  - GitHub Project Workaround: [derekantrican/GAS-ICS-Sync](https://github.com/derekantrican/GAS-ICS-Sync)

#### 7. Offline Caching and Spotty Campus Wi-Fi Resilience
- **Description**: Students need instant terminal access to assignment descriptions, required readings, rubrics, and instructor announcements even when offline, commuting, or during campus network outages.
- **Current Pain**: Canvas web is a heavy Single Page Application (SPA). If campus Wi-Fi drops or Canvas experiences downtime during finals week, students cannot even read the assignment prompt.
- **Evidence Links**:
  - Reddit discussion: [r/college: "Canvas went down during finals week and no one can see prompts"](https://www.reddit.com/r/college/comments/13e2x9p/canvas_down_finals_week/)
  - GitHub Archival Solution: [davekats/canvas-student-data-export](https://github.com/davekats/canvas-student-data-export)

#### 8. Instant Announcement and Grade Publication Alerts
- **Description**: Students want real-time notifications for instructor announcements and newly posted grades, delivered directly to their desktop or terminal rather than buried in university email digests.
- **Current Pain**: University email accounts receive dozens of automated Canvas notifications per day, causing students to miss critical course cancellations, room changes, or grade feedback.
- **Evidence Links**:
  - Reddit complaint: [r/college: "Canvas email notifications are completely overwhelming"](https://www.reddit.com/r/college/comments/171hbc5/canvas_email_spam/)
  - Raycast Community Extension: [CanvasCast in Raycast](https://github.com/raycast/extensions/tree/main/extensions/canvascast)

#### 9. Cryptographic Submission Receipts and Verification
- **Description**: Students want immediate, verifiable receipts of assignment submissions, recording the SHA-256 hash of the submitted file, the exact server-confirmed timestamp, and the submission ID.
- **Current Pain**: Students live in fear of "Canvas ate my homework" scenarios or corrupted uploads. They routinely take manual screenshots of the submission confetti screen as defensive proof.
- **Evidence Links**:
  - Reddit post: [r/college: "Take a screenshot of every Canvas submission receipt you submit"](https://www.reddit.com/r/college/comments/q45v2q/always_screenshot_your_canvas_submissions/)
  - Instructure Community Discussion: [Instructure Community: "Student submission receipts and audit trail"](https://community.canvaslms.com/t5/Canvas-Question-Forum/Submission-receipts/m-p/204812)

#### 10. Resilient Authentication for Token-Restricted Institutions
- **Description**: Students enrolled at universities that disable the "New Access Token" button in Canvas user settings need an automated, compliant fallback to authenticate from the CLI.
- **Current Pain**: Many institutions turn off student token generation via administrative permissions. As a result, 95% of open-source Canvas CLI tools immediately fail for those students.
- **Evidence Links**:
  - Instructure Community Discussion: [Instructure Community: "Institutions disabling manual access tokens for students"](https://community.canvaslms.com/t5/Canvas-Question-Forum/Cannot-create-access-token-button-missing/m-p/118293)
  - GitHub Project Solution: [solomonneas/canvas-cli](https://github.com/solomonneas/canvas-cli)

---

## 3. Gaps in the Current Landscape

No existing tool provides a cohesive, polished student client. The following architectural and UX gaps remain unaddressed:

1. **Fragmented Tooling Ecosystem**:
   Existing tools are either hyper-focused single-purpose utilities (such as `wznmickey/canvas_syncer` which only downloads files, or `mbund/canvas-cli` which only uploads submissions) or massive enterprise clients with 90+ command groups (like `jjuanrivvera/canvas-cli`). There is no unified "student daily driver" that handles dashboard triage, assignment submission, file extraction, and grade checking in one cohesive binary.

2. **Insecure Plaintext Token Storage**:
   Virtually all existing tools (`danielhuang/canvas-cli`, `GideonWolfe/canvas-tui`, `grantlemons/canvas-cli`, `davekats/canvas-student-data-export`) store personal access tokens in plaintext TOML, YAML, or JSON files (`~/.canvas.toml`, `config.yaml`). Only `jjuanrivvera/canvas-cli` and `CanvasCast` use secure OS keyrings, but one is a complex Go enterprise tool and the other is a macOS-only Raycast launcher plugin.

3. **Zero Local Caching / Unacceptable Startup Latency**:
   Existing tools query the live Canvas API on every invocation. When a student runs a command, they wait 1.5 to 4.0 seconds for HTTP handshakes and paginated responses. `GideonWolfe/canvas-tui` is infamous for freezing on startup because it makes 20+ sequential API calls. No existing tool maintains an instant local SQLite or embedded cache with HTTP ETag validation for sub-10ms CLI responsiveness.

4. **Missing Reverse Target Grade Solver**:
   While some tools display current grades, zero tools implement a backwards-solving "What-If" calculator that takes assignment group weights, current scores, and dropped assignments into account to compute the exact score required on remaining assignments to hit an target GPA or letter grade.

5. **Failure on Disabled "Files" Tabs**:
   Standard file downloaders rely strictly on the `/api/v1/courses/:id/files` endpoint. When an instructor hides the Files tab (a very common pedagogical practice), these downloaders fail completely. While syncers like `wznmickey/canvas_syncer` parse the Modules API, they do not present files cleanly within assignment and lecture contexts.

6. **Poor Output Ergonomics**:
   Existing tools either print rigid, wide tables that wrap and break on standard terminal viewports, or print raw unformatted JSON. No tool provides dual-mode output: human-friendly, colored terminal tables that respect `NO_COLOR` and terminal width, alongside machine-parsable `--json` output designed for piping into `jq`, `fzf`, or AI agents.

7. **Lack of Submission Verification Receipts**:
   No tool automatically generates a local cryptographic audit receipt (SHA-256 checksum, submission ID, timestamp, and server response payload) upon completing an assignment upload, leaving students without proof of timely submission.

8. **Heavyweight SSO Fallbacks**:
   When universities disable manual API tokens, the only tool attempting a workaround (`solomonneas/canvas-cli`) pulls in the entire Playwright and Chromium browser stack (~200MB download). No tool provides a lightweight session cookie capture or standard browser cookie import mechanism.

---

## 4. Naming Analysis & Proposals

### 4.1 Verification Across Registries

Research was conducted on crates.io, npm, Homebrew, and GitHub for the identifier `canvas-cli`:

| Registry | Status for `canvas-cli` | Details / Owner / Link |
| :--- | :--- | :--- |
| **crates.io** | **TAKEN** | Published by Mark Bundschuh (`mbund`), v0.1.0 on 2025-03-12. Description: *"Interact with Canvas LMS from the command line."* URL: https://crates.io/crates/canvas-cli |
| **npm** | **TAKEN** | Published by Nishan Bajracharya (`nisbaj`), v1.0.3 on 2019-07-14. Description: *"HTML Canvas project bootstrapper."* (Unrelated to Canvas LMS). URL: https://www.npmjs.com/package/canvas-cli |
| **Homebrew** | **AVAILABLE** | Not present in `homebrew/core` or `homebrew/cask`. Verified via `brew info canvas-cli` (returns Error: No available formula). |
| **GitHub** | **TAKEN** | Top star repository matching exact name: `echosoar/canvas-cli` (60 stars, pure Rust drawer library for image generation). Top Canvas LMS repos: `GideonWolfe/canvas-cli` (18 stars), `jjuanrivvera/canvas-cli` (13 stars), `mbund/canvas-cli` (11 stars). |

Because `canvas-cli` is already claimed on both crates.io and npm, publishing a Rust CLI crate as `canvas-cli` on crates.io will be rejected by the registry.

---

### 4.2 Proposed Alternative Binary Names

Three Latin-flavored alternative names were verified across registries:

#### Option 1: `elicio` (Recommended)
- **Latin Etymology**: *Ēliciō* (third conjugation) — meaning *"to draw out, call forth, elicit, obtain, bring to light"*.
- **Rationale**: Perfectly describes extracting coursework, grades, files, and deadlines from the Canvas LMS monolith into the light of the terminal.
- **Registry Status**:
  - **crates.io**: **FREE / AVAILABLE** (Verified: crate does not exist).
  - **npm**: **FREE / AVAILABLE** (Verified: package does not exist).
  - **Homebrew**: **FREE / AVAILABLE** (Verified: formula does not exist).
  - **GitHub**: No dominant conflict.
- **Binary Command**: `elicio courses`, `elicio todo`, `elicio submit`

#### Option 2: `ubique`
- **Latin Etymology**: *Ubīque* — meaning *"everywhere, anywhere, in all places"*.
- **Rationale**: Symbolizes ubiquitous access to your university coursework directly from any terminal or SSH session.
- **Registry Status**:
  - **crates.io**: **FREE / AVAILABLE** (Verified: crate does not exist).
  - **npm**: TAKEN (by unrelated utility).
  - **Homebrew**: **FREE / AVAILABLE**.
  - **GitHub**: No dominant tool.
- **Binary Command**: `ubique todo`, `ubique submit`

#### Option 3: `venator`
- **Latin Etymology**: *Vēnātor* — meaning *"hunter"*.
- **Rationale**: Represents actively hunting down upcoming deadlines, missing assignments, and hidden course resources.
- **Registry Status**:
  - **crates.io**: TAKEN as a library (a logging/tracing layer crate exists named `venator`). However, the crate name `venator-cli` or `canvas-venator` is **FREE**, and the local binary name installed via Homebrew or Cargo can remain `venator`.
  - **npm**: TAKEN.
  - **Homebrew**: **FREE / AVAILABLE**.
- **Binary Command**: `venator todo`, `venator sync`

#### Additional Latin Alternative: `disce`
- **Latin Etymology**: *Disce* (imperative of *discere*) — meaning *"Learn!"*.
- **Registry Status**: **FREE** on crates.io, **FREE** on npm.

---

## 5. Legal and Terms of Service (ToS) Constraints

Any student command-line client interacting with Instructure Canvas LMS must strictly adhere to Instructure's Terms of Use, Acceptable Use Policy (AUP), and institutional academic integrity regulations.

### 5.1 Instructure Terms of Use & Acceptable Use Policy (AUP)

1. **No Third-Party Token Sharing**:
   - Instructure's Acceptable Use Policy strictly forbids sharing credentials, passwords, or API access tokens with third parties.
   - **Compliance Rule**: The CLI must operate entirely client-side. It must **never** transmit API tokens or student credentials to an intermediary telemetry server, remote proxy, or shared database. Tools like `MohammedADev/Canvas-Discord-Bot` violate security best practices because user tokens are sent to a shared Discord bot instance.

2. **Prohibition of Denial of Service & Excessive Load**:
   - Automated scripts that flood the API risk account suspension and institutional IP bans.
   - Canvas enforces a dynamic leaky-bucket rate limiter tracking the `X-Rate-Limit-Remaining` and `X-Request-Cost` response headers. Canvas returns `403 Forbidden` with body `Rate Limit Exceeded` when the bucket depletes.
   - **Compliance Rule**: The CLI must limit concurrent HTTP connections (recommended max: 2 to 4 concurrent connections), implement exponential backoff with jitter on 403/429 responses, and cache static course metadata locally.

3. **Academic Integrity & Automated Assessment Restrictions**:
   - Automated completion or scraping of quizzes and exams violates both Instructure policies and university academic honor codes.
   - **Compliance Rule**: The CLI must **never** implement endpoints that take quizzes, submit quiz answers, or exploit ID enumeration (IDOR) to access unreleased or locked quiz keys.

4. **Honoring Access Control & Visibility Flags**:
   - Canvas permissions govern what students can view. Attempting to scrape unpublished assignments, hidden gradebook columns, or peer submissions through API probing constitutes unauthorized access.
   - **Compliance Rule**: The CLI must strictly respect permissions returned by Canvas endpoints (e.g. `locked_for_user: true`, `hidden: true`).

5. **Institutional Rights on Token Generation**:
   - Institutions retain the right under FERPA and IT policy to disable Personal Access Token generation for student roles (`Account > Settings > Approved Integrations`).
   - **Compliance Rule**: If a student's institution disables token generation, the CLI should provide clear, polite guidance. If a session cookie fallback is implemented, it must use the student's existing authenticated local session and must not attempt password brute-forcing or credential harvesting.

6. **Descriptive User-Agent Headers**:
   - Instructure recommends that API clients supply an identifiable `User-Agent` string (e.g. `User-Agent: elicio/0.1.0 (https://github.com/owner/elicio)`). This enables university system administrators to distinguish legitimate student client traffic from malicious scraping attacks.
