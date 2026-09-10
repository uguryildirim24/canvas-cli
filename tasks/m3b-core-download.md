# M3-b-core — Download core, I/O bridge, markdown, ICS in `crates/canvas-core` (Cursor Auto, lane w3)

Read `docs/SPEC.md` §12.3 (layout, sanitization, containment, manifest,
install critical section, clobber table, move protocol, outcome mapping),
§12.5 (ICS rules), §13 (blocking I/O bridge), §16 row 2 (tests), Appendix A.
Pure local logic: the network transport is a trait you define; no HTTP here.

## Deliverables
1. `canvas_core::download::plan`: input = course code/id, module list with
   positions and file items, folder tree and file listing (define plain input
   structs; do not depend on `canvas-api` models yet). Output = one planned
   path per file with ownership per §12.3 (lowest module position, then
   lowest module id; Files-listing files under `files/<folder path>/`).
   Planning is independent of `--module`/`--file` filters.
2. `canvas_core::download::sanitize`: the per-component rules in §12.3
   (invalid chars, trailing spaces/dots, `.`/`..`, leading dot, empty →
   `file-<id>`, Windows device names incl. `COM0`–`COM9`, `LPT0`–`LPT9`, and
   superscript ¹²³ forms, 180-byte UTF-8-boundary truncation leaving room for
   `-<file_id>`), then the **uniqueness pass** over the whole planned set
   (case-insensitive after NFC; suffix `-<file_id>`; repeat until unique;
   generated names count as reserved; deterministic).
3. `canvas_core::download::contain`: open the destination root as
   `cap_std::fs::Dir`; walk components with
   `cap_fs_ext::DirExt::open_dir_nofollow` (create if absent), retain every
   handle; final entry inspected with no-follow metadata via the parent handle;
   open final files with `FollowSymlinks::No` and keep the descriptor for the
   whole validation; refuse symlinks/reparse points anywhere (`unsafe_path`);
   temp files `.<name>.<random>.part` via `create_new` in the parent handle;
   install via `Dir::rename` parent → parent.
4. `canvas_core::download::manifest`: `<dest>/.canvas-cli/manifest.sqlite`
   rows `(file_id, course_id, path, size, sha256?, remote_updated_at,
   installed_at, pending_move_to?)`; `dest.json` create/verify; `install.lock`
   exclusive mutex (`fs4`) + in-process `tokio::sync::Mutex`; 30 s timeout →
   `lock_timeout`.
5. `canvas_core::download::install`: the clobber table and the install
   critical section (re-read row → classify → rename → manifest commit after
   rename), `--force` semantics, the move protocol with `pending_move_to`
   and its startup resolution, and the per-file `Action` enum matching
   Appendix D `download@1` (`planned|downloaded|moved|skipped|unmanaged|
   modified|locked|unavailable|skipped_external|unsafe_path|failed`) plus the
   outcome mapping helper (which actions make the run `partial`).
   Transport = `trait Transfer { async fn fetch(&self, file_id, sink, expected_size) -> Result<u64> }`
   with a fake for tests.
6. `canvas_core::io`: blocking bridge per §13 — `spawn_blocking` workers for
   cap-std operations, `fsync`, SHA-256 hashing; bounded channels of 64 KiB
   chunks with backpressure between blocking and async sides.
7. `canvas_core::markdown`: HTML → Markdown/plain text for assignment
   descriptions and announcements (pick `htmd` or `html2text`; note the choice).
8. `canvas_core::ics`: RFC 5545 writer per §12.5 — `VCALENDAR` with
   `PRODID`/`VERSION`, `VEVENT` with `UID`, `DTSTAMP`, `DTSTART` UTC or
   `DTSTART;VALUE=DATE` with **no `DTEND`** for all-day, `DTEND` for timed
   events with an end, `SUMMARY`, `URL`, `DESCRIPTION`, text escaping
   (§3.3.11), CRLF, 75-octet folding, optional `VALARM` before deadlines.
   Input = a plain `CalendarItem` struct you define.
9. Tests (§16 row 2, download/ICS part): sanitization incl. Windows names and
   superscripts; generated-name vs literal-name collision; empty component;
   containment against `..`, absolute paths, in-root symlinked parent,
   symlinked final path, symlink swapped between inspection and open; every
   clobber-table row incl. first-run unmanaged file and two competing
   installers (two processes); modified-owned file; move with remote
   unchanged / remote changed / target occupied; `pending_move_to` recovery
   (new exists, old exists, neither); ICS escaping, folding, all-day
   west-of-UTC, all-day across DST, equal start/end all-day.

## Rules
- Owner files: `crates/canvas-core/src/{download,io,markdown,ics}/**`, their
  `lib.rs` module lines, and the `canvas-core/Cargo.toml` dependencies you
  need (cap-std, cap-fs-ext, fs4, rusqlite bundled, sha2, unicode-normalization,
  jiff, tokio, the HTML crate). Do not touch `store`, `identity`, other
  crates, `docs/`, or `tasks/`. The manifest DB is separate from the
  identity databases; do not use `canvas_core::store`.
- Do NOT `git commit`. Leave the tree for review.

## Gates (all must pass before you report)
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
```

Finish with `git status --short` and reply exactly: `DONE M3-b-core`
