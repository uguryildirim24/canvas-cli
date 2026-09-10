# R2 — Prior art and student needs (research only, no code)

Goal: what already exists for Canvas LMS (Instructure) on the command line and
what students actually want. Use Google, GitHub, crates.io, npm, PyPI, Reddit.

Write findings to `docs/research/r2-prior-art.md` in this repo. Markdown with a
comparison table and a gaps list. Cite URLs. Mark unverified items UNVERIFIED.
Do not write any code files.

Cover:
1. Existing tools (aim for 10–20). For EACH: name, URL, language, last commit
   date, stars, auth method, token storage approach, feature list (courses,
   assignments, grades, submit, file download, modules, announcements, calendar
   export, TUI), output style (table/JSON), and one line of what is good and one
   line of what is bad. Include at least: npm `canvas-cli` packages, Python
   `canvasapi` (library) and CLIs built on it, any Rust crates for Canvas LMS on
   crates.io / lib.rs (search "canvas lms", "instructure"), Go clients,
   Obsidian/Emacs/Neovim plugins, Raycast/Alfred extensions, "canvas file
   downloader" tools, canvas-to-ICS/Google Calendar sync tools, Canvas Discord
   bots. Note official Instructure SDKs (if any).
2. Student needs. Mine Reddit (r/college, r/Canvas, r/commandline), GitHub issues
   of the tools above, Instructure Community idea posts. List the top 10 wants
   with evidence links: e.g. due-date dashboard, bulk file download, grade
   "what-if", submit from terminal, missing-assignment alerts, calendar/todo
   sync, offline cache, notifications.
3. Gaps: what no existing tool does well.
4. Naming: check whether `canvas-cli` is taken on crates.io, npm, Homebrew,
   GitHub (top result). Propose 3 short alternative binary names; Latin-flavored
   names are welcome (owner likes them: elicio, ubique, venator).
5. Legal/ToS: Instructure's terms on personal access tokens and automated
   access; anything a student tool must avoid.

Finish by replying in chat with exactly: `DONE docs/research/r2-prior-art.md`.
