# Code review — M8-a, richer reading context (lane `lane/w1`)

## Verdict

**MERGE.** The package matches its brief, `docs/reads-v2.md`, REPORT §4's
M8-a acceptance column, and SPEC §7, §10, §14, and §15 after six
`review(M8-a):` commits on this branch. Five defects were found and fixed in
the tree: three of them (a capability-bearing URL reaching the cache and the
JSON, a cut reply body reported at exit 0, a rubric row cached before M8-a
losing three declared fields) were contract violations, not cosmetics.
Nothing is pushed and nothing is merged into `main`; three items are listed
under "Needs a decision" and none of them blocks this package.

## Gates

Run with `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m8a`,
first on the delivered branch and again after every fix.

| Gate | As delivered | After the fixes |
|---|---|---|
| `cargo fmt --all --check` | clean | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean | clean |
| `cargo nextest run --all-features` | 606 passed, 0 skipped | 611 passed, 0 skipped |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok | same |
| `cargo +1.88 check --workspace --all-targets` | clean | clean |
| `cargo xtask bench --runs 3` | every §13 target met | every §13 target met |

Bench p95 after the fixes, 5-course set: cached todo first output 7.4 ms
idle / 7.7 ms download (target 150); full run 8.2 / 8.4 (target 250); cold
start 10.1 / 10.6 (target 400). `docs/bench.md` is rewritten by the gate and
was restored each time, so it stays as `main` has it.

## What was checked and holds

- **Verb and route.** Every request in the package is a `GET`; the wiremock
  request log is asserted for the method on five paths, and the
  materialized `/view` discussion endpoint appears nowhere but in a comment
  saying why it is not used.
- **Nothing is marked read.** `inbox`, `inbox show`, and the conversation
  refresh all carry `auto_mark_as_read=false`, asserted over the whole
  request log; no request carries `auto_mark_as_read=true`.
- **Denials.** A `pages`/`discussions`/`inbox` listing denial is a
  `partial[]` row with `listing.available = false` at exit 12, stored as
  coverage so a cached read repeats it; a single item denied is exit 8.
- **The initial-post gate** refuses `--replies` at exit 8 and still reads
  the topic at exit 0; a `403` on a topic without the flag stays an ordinary
  incomplete page set.
- **Reply coverage.** A read without `--replies` reports
  `blocked: not_requested` and never `complete`, and sends no request to the
  entries route. A failed second page reports `complete: false`,
  `blocked: page_failed`, and exit 12.
- **Embedded content and file references.** Frames, LTI launches, video and
  audio become `embedded` rows with `reported: unavailable` and are never
  fetched; cross-origin references are listed as `external_links` and never
  fetched; same-origin `/files/:id` references become `files` rows.
- **Cache holds no raw HTML for the syllabus.** `courses.data_json` keeps
  `syllabus_markdown` and the JSON `syllabus_refs` projection, asserted to
  contain neither `<p>` nor `<iframe>`.
- **Schemas.** All eight are registered with fixtures, list every field with
  no `skip_serializing_if`, use `null` for unknown, and never make an array
  nullable. README and clap stay in parity.
- **Dependencies.** No `Cargo.toml` or `Cargo.lock` change, so Appendix A
  and MSRV 1.88 are untouched.

## Defects found and fixed

| # | Sev | Where | What was wrong | What I changed | Commit |
|---|---|---|---|---|---|
| 1 | High | `crates/canvas-core/src/markdown/extract.rs:229`, `crates/canvas-core/src/sync/pages.rs:234`, `crates/canvas-core/src/sync/discussions.rs:344` | A reference was stored and printed exactly as written. A Canvas body links to signed and `verifier`-bearing URLs, so a capability reached `pages.data_json`, `discussion_topics.data_json`, the `syllabus_refs` projection in `courses.data_json`, the converted Markdown body, and every `files`/`external_links` row in the JSON. §15 keeps a capability off disk and out of the output, and `modules` already stripped one from its `html_url`. | Sanitize each reference where it is collected, rewriting it in the tree so the rendered Markdown loses it too: drop the userinfo and every query parameter §15 names, keep the rest of the reference. `html_url` on pages and topics now goes through the `modules` rule. New `canvas_api::redact::is_capability_key`, one unit test, one end-to-end test that reads the cache back. | `350edc2` |
| 2 | High | `crates/canvas-cli/src/commands/discussions.rs:298` | Reply bodies were bounded at 64 KiB and carried `truncated: true`, but only the topic message raised a `partial[]` row. A thread whose replies were cut came back at exit 0 with `outcome: ok` — the one thing the package must never do with a cut body. The conversation path already reported its cut messages. | Raise a `partial[]` row under `discussion_entries:topic:<id>` and exit 12 when any shown reply was cut. New test. | `2de3ce4` |
| 3 | Medium | `crates/canvas-cli/src/commands/assignment.rs:95`, `crates/canvas-cli/src/commands/submission.rs:619` | The rubric extension is additive on the wire but only a fresh fetch wrote the added keys. A `rubric_json` or `rubric_assessment_json` row cached by a pre-M8-a build still came back with the v1 keys alone, so `assignment@1` printed criteria without `long_description`, `criterion_use_range`, and `ratings[]`, and `submission@1` printed assessments without `rating_id`, until the row's TTL expired. §7 says a declared field is always present. | Re-project both on read through the same functions that write them (`criterion_json`, new `assessment_row_json`), and cover a cache rewritten to the pre-M8-a shape. | `762e797` |
| 4 | Medium | `crates/canvas-api/src/models/conversation.rs:72` | `unread_count` was `Option<String>`, so a Canvas that answers `{"unread_count": 7}` turned the whole `inbox unread-count` read into a decode failure. The package's own decision is that an unknown count stays `null`, and every id-shaped field in `canvas-api` already accepts a number or a string. | Accept a string or a number, keep `null` for absent and unparseable. Test over all four shapes. | `7f77f98` |
| 5 | Low | `crates/canvas-cli/src/commands/discussions.rs:316` | The initial-post gate emitted `code: "denied"`. Every other exit 8 in the workspace, the single-item denial in this same package included, emits `code: "refused"`, and §14 names the code "Refused". Two codes for one exit is a trap for an agent that branches on `code`. | Emit `"refused"`, keep the `initial_post_required:` message prefix, update the test and the exit table in `docs/reads-v2.md`. | `9c814fd` |
| — | — | `docs/reads-v2.md` | The contract document did not describe the capability rule, and its truncation row named only the body. | Record what the code now does: which parts of a reference are stripped and when, and that the 64 KiB bound counts per document, a reply and a conversation message included. | `375bab9` |

Test count moved 606 → 611: one unit test in `canvas-core::markdown::extract`
and four integration tests in `crates/canvas-cli/tests/review_m8a.rs`.

## Needs a decision

1. **Raw HTML bodies stay in the cache.** `pages.body`,
   `discussion_topics.message`, and `discussion_entries.message` hold the
   Canvas HTML, converted to Markdown only at read time. §15 says "no raw
   response bodies on disk; allowlisted records only", and M1-b reads that
   as converting a body before storing it, which is what the syllabus does.
   This package follows the v1 `announcements.message` pattern already on
   `main` instead, so it is consistent with the codebase and not a
   regression. Deciding it one way means converting at ingest for pages,
   discussions, **and** announcements, which changes a v1 dataset and the
   64 KiB bound's placement; deciding it the other way means writing down
   that an allowlisted body field may be stored as sent. I did not guess.
2. **`discussions --announcements yes` may promise more than the pinned
   endpoint returns.** The brief fixes the request at
   `?only_announcements=false`, and the flag filters the stored
   `is_announcement` locally. Canvas's own discussion-topics index is
   documented and widely reported to return non-announcement topics on that
   route, in which case the `yes` default can never show an announcement and
   the flag only ever removes nothing. I could not verify this against a
   live Canvas from here, and the fix — a second request, or dropping the
   default's promise and pointing at `canvas announcements` — changes the
   contract the brief pinned.
3. **`discussion --replies --page N` past the end of the stored set**
   returns `replies: []` beside `replies_coverage.complete = true`. The
   coverage is honest about the fetch, as decision 12 of `docs/reads-v2.md`
   says, but `discussion@1` carries no window fields, so a reader cannot
   tell an empty page from an empty thread. Either add `replies_page` and a
   total to the schema (a `@1` field addition, so the version holds), or
   make an out-of-range window exit 2 the way `--page 0` already does.

## Scope not counted as missing

Item 3 (the handler extraction and the eight MCP tools) and item 4 (the
`inbox.unread_count` event) wait for M6-b and M6-c on `main`, per the review
brief. `git log main` shows neither `merge lane/w2: M6-b` nor
`merge lane/w3: M6-c`, and both are recorded as left for the next round in
`docs/reads-v2.md`.
