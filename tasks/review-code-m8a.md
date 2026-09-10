# Code review + fix — M8-a on branch lane/w1 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M8-a
on branch `lane/w1` (worktree `/home/user/projects/canvas-cli`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /home/user/projects/canvas-cli`. Read the package brief `tasks/m8a-richer-reads.md` and the SPEC sections it
   cites. First run `git merge main` (expect nothing to do; resolve if needed and commit). Then read `git log main..HEAD --stat` and the full diff. Scope: the package is delivered without its item 3 (MCP tools) and item 4 (the `inbox.unread_count` event), which wait for M6-b and M6-c on `main` and will be a separate follow-on; do not count them as missing. Contract: the brief's own contract table and `docs/reads-v2.md` as built, REPORT §4's M8-a acceptance column, and `docs/SPEC.md` §7, §10, §14, §15. Attack in particular: does any request in the package use a verb other than `GET`, omit `auto_mark_as_read=false` where the table sets it, or touch the `/view` materialized endpoint (grep the wiremock request log in the tests, then the client code)? Is anything ever marked read? Do listings and single items report denial, the initial-post gate, and a failed reply page exactly as the brief's failure rules say, and never label a cut or partial result complete? Are embedded frames, LTI launches, video and audio reported and never fetched; are cross-origin file references listed as external and never fetched; do same-origin file references keep the origin and containment rules? Does the Markdown conversion store no raw HTML body in the cache (§15, M1-b rule) and does every body stop at 64 KiB on a character boundary with `truncated: true`? Is the `assignment@1` rubric extension purely additive (old fixtures validate)? Does every new schema list every field, always present, `null` for unknown, arrays never `null`? Rerun `cargo xtask bench --runs 3` and confirm the §13 targets. The worker's final report, for reference:

```
# Lane w1 — M8-a final report

**Package:** M8-a, richer reading context: pages, syllabus, rubric, discussions, inbox reads.
**Branch:** `lane/w1` in `/home/user/projects/canvas-cli`. Not pushed. Not merged into `main`.
**Marker:** `WAITING M6-b` (brief item 3: `git log main` shows neither `merge lane/w2: M6-b` nor `merge lane/w3: M6-c`).

Items 1, 2, 4, 5, 6, and 7 are complete. Item 3 is blocked and is listed under "Left for the next round".

---

## 1. Commits on the lane

```
4f5a274 test: drop a stray insta .snap.new left by a forced update
a1f0fed Merge branch 'main' into lane/w1
1d65cc1 docs(reads-v2): record the M8-a read contract as built
67a7bf0 test(m8a): cover the read contract, coverage honesty, and the exits
af6bd8f test(bench): add the M8-a read surface to the bench-5 fixture set
ace7f5b feat(cli): add pages, syllabus, discussions, and inbox reads
31117e8 feat(core): add pages, discussions, and inbox datasets
```

`git status --short` is empty.

---

## 2. Delivered items

### Item 1 — datasets, refresh functions, config keys

New modules in `canvas-core::sync`:

| Module | Datasets |
|---|---|
| `sync/pages.rs` | `PagesDataset`, `PageDetailDataset`, `refresh_pages`, `refresh_page` |
| `sync/discussions.rs` | `DiscussionsDataset`, `DiscussionDataset`, `refresh_discussions`, `refresh_discussion` |
| `sync/inbox.rs` | `InboxDataset`, `ConversationDataset`, `InboxUnreadDataset`, `refresh_inbox`, `refresh_conversation`, `refresh_inbox_unread` |

New API models in `canvas-api::models`: `WikiPage`, `DiscussionTopic`, `DiscussionEntry`, `GroupTopicChild`, `Conversation`, `ConversationParticipant`, `ConversationMessage`, `ConversationAttachment`, `UnreadCount`.

Config keys, with the M4-b keys as the model: `cache.ttl_pages` (default `1h`), `cache.ttl_discussions` (default `15m`), `cache.ttl_inbox` (default `5m`). Each is in the `CacheConfig` struct, the defaults, the `KNOWN_TOP` `config set` allowlist, and the `apply_set` match, with a `ttl_*()` accessor in `session.rs`.

**Cache migration `0002_reads`**, `CACHE_USER_VERSION` moved 1 → 2. The brief said no migration was expected; these entities have no v1 table, so 2 is the next free number. New tables, all keyed by the Canvas id and all carrying `observed_at_core/detail/status`: `pages`, `discussion_topics`, `discussion_entries`, `conversations`, `conversation_unread`. Added to `CACHE_TABLES`.

### Item 2 — commands, README rows, rubric extension

Eight commands, in table and `--json` mode, with clap entries in `cli.rs`, dispatch in `main.rs`, and modules `commands/{pages,discussions,inbox}.rs`:
`pages`, `page`, `syllabus`, `discussions`, `discussion`, `inbox`, `inbox show`, `inbox unread-count`.

Eight schemas registered with fixtures and listed in the e2e `SHAPES` table:
`pages@1`, `page@1`, `syllabus@1`, `discussions@1`, `discussion@1`, `inbox@1`, `conversation@1`, `inbox_unread@1`.

README `## Commands` gained eight rows.

`assignment@1` extended additively: a rubric criterion keeps `id`, `description`, `points` and gains `long_description`, `criterion_use_range`, `ratings[{id, description, long_description, points}]`; a rubric assessment gains `rating_id`. Nothing existing is renamed.

New shared body projection `canvas-core::markdown::extract`: `rich_text`, `rich_text_opt`, `BodyRefs`, `RawEmbed`, `RawLink`, `ResolvedRefs`, `EmbeddedRef`, `FileRef`, `ExternalLink`, `BODY_LIMIT` (64 KiB).

### Item 5 — fixtures

`bench-5` gained 13 responses: a two-page listing and one page body (with an iframe, an LTI iframe, a same-origin `/files/:id` link, and a cross-origin link), three discussion topics (plain, initial-post gated, graded group) with their details, a reply set, a `403` on the gated entries route, three conversations with details, and the unread count. The MANIFEST gained 13 endpoints. The diff is additions only.

Wiremock acceptance fixtures cover the same surface plus two paginated reply pages with a `Link: rel="next"` header, served or refused.

### Item 6 — tests

New file `crates/canvas-cli/tests/review_m8a.rs`, 12 tests, one per REPORT §4 acceptance row:

| Test | Covers |
|---|---|
| `a_page_reports_its_embedded_content_files_and_external_links` | embedded reported, same-origin files only, cross-origin listed and never fetched, `--unpublished`, GET-only log, page/pages snapshots in both modes |
| `a_denied_listing_is_partial_and_a_denied_page_is_refused` | listing denial `partial[]` exit 12 (fresh and cached), single-item denial exit 8 |
| `paginated_replies_report_complete_and_incomplete_coverage` | two reply pages complete, and a failed second page reporting `complete: false` with exit 12; discussion snapshots in both modes |
| `a_topic_read_without_replies_never_claims_the_thread_is_covered` | `blocked: not_requested`, and no request to the entries route |
| `the_initial_post_gate_refuses_replies_and_still_reads_the_topic` | topic reads at exit 0; `--replies` is exit 8 `initial_post_required` |
| `a_graded_group_discussion_keeps_its_metadata` | `assignment_id`, `points_possible`, `group_category_id`, `group_topic_children`; `--announcements no`, `--unread`, bad flag exit 2; discussions snapshots in both modes |
| `every_inbox_request_refuses_to_mark_anything_read` | `auto_mark_as_read=false` on every conversation request, GET-only log, inbox/conversation/unread snapshots in both modes |
| `a_bad_inbox_scope_is_a_usage_error_and_sends_nothing` | exit 2, no request sent |
| `the_syllabus_reports_its_markdown_and_the_files_it_links` | syllabus refs, and the cache holding no raw HTML; snapshots in both modes |
| `a_body_over_the_bound_is_cut_and_never_called_complete` | 64 KiB bound, `truncated: true`, `partial[]`, exit 12 |
| `a_url_operand_must_name_the_course_it_was_given` | URL form accepted; wrong course and cross-origin both exit 6 |
| `an_old_assignment_rubric_still_validates` | the additive rubric change against a pre-M8-a payload |

README/clap parity is enforced by the existing `tests/readme.rs`. Six unit tests were added in `canvas-core::markdown::extract`, including one asserting the stored projection carries no markup.

### Item 7 — `docs/reads-v2.md`

187 lines: the contract table as built, the exact endpoints and query strings, the datasets and TTLs, the cache shape, the schemas, the body/embedded/file rules, the exit table, the nineteen decisions below, and the two items left for the next round. This is the only file under `docs/` I wrote.

---

## 3. The contract as built

All commands are class C. Every request is a `GET`. Nothing in this package marks anything read.

| Command | Request | Dataset / scope / TTL |
|---|---|---|
| `pages <course> [--unpublished]` | `GET /api/v1/courses/:id/pages?sort=title&per_page=100` | `pages` / `course:<id>` / `ttl_pages` (1h) |
| `page <course> <url-slug\|id\|URL>` | `GET /api/v1/courses/:id/pages/:url_or_id` | `page` / `page:<course>:<operand>` / `ttl_pages` |
| `syllabus <course>` | none new; the `course` dataset already carries the syllabus | `course` / `course:<id>` / `ttl_courses` |
| `discussions <course> [--unread] [--announcements yes\|no]` | `GET /api/v1/courses/:id/discussion_topics?only_announcements=false&per_page=100` | `discussions` / `course:<id>` / `ttl_discussions` (15m) |
| `discussion <course> <id\|URL> [--replies] [--page N]` | `GET /api/v1/courses/:id/discussion_topics/:tid`; with `--replies` also `GET …/discussion_topics/:tid/entries?per_page=100` and `GET …/entries/:eid/replies?per_page=100` | `discussion` / `topic:<id>` or `topic:<id>:replies` / `ttl_discussions` |
| `inbox [--scope inbox\|unread\|sent\|archived]` | `GET /api/v1/conversations?scope=<scope>&auto_mark_as_read=false&per_page=100` | `inbox` / `scope:<scope>` / `ttl_inbox` (5m) |
| `inbox show <id>` | `GET /api/v1/conversations/:id?auto_mark_as_read=false` | `conversation` / `conversation:<id>` / `ttl_inbox` |
| `inbox unread-count` | `GET /api/v1/conversations/unread_count` | `inbox_unread` / `all` / `ttl_inbox` |
| `assignment` (extended) | unchanged requests | unchanged |

`per_page=100` is appended by `canvas_api::Client` when a path does not already carry it. The `/view` materialized discussion endpoint is never used: it marks entries read as a side effect of reading them.

### Bodies, embedded content, file references

`rich_text` converts one HTML body and returns the Markdown plus a `BodyRefs` projection that holds no HTML.

- `<iframe>`, `<video>`, `<audio>`, `<embed>`, `<object>` each become an `embedded` row and a one-line placeholder in the Markdown. `kind` is `video`, `audio`, `lti` (an iframe whose source names `external_tools` or `/lti/`), `iframe`, or `unknown`. `reported` is always `unavailable`.
- `<a href>`, `<area href>`, `<img src>`, `<source src>` resolve against the identity origin at read time. Same-origin and ending in `/files/:id` becomes a `files` row; any other origin becomes an `external_links` row and is never fetched. Fragments, `mailto:`, and `javascript:` are dropped.
- Over 64 KiB the body is cut on a character boundary, `truncated` is `true`, the envelope gains a `partial[]` row, and the exit is 12.

### Exits

| Case | Outcome |
|---|---|
| Listing denied (`403`/`404`, not a throttle) | `partial[]` row `pages:course:<id>`, `discussions:course:<id>`, or `inbox:scope:<scope>`; `listing.available = false`; exit 12. Stored as coverage, so a cached read reports it too. |
| Single item denied | exit 8, `code: refused` |
| Not found | exit 6 through the resolver path |
| Cross-origin URL operand | exit 6, `code: resolution` |
| `require_initial_post` gate with `--replies` | exit 8, `code: denied`, message starting `initial_post_required` |
| A reply page failed after earlier pages were stored | `replies_coverage.complete = false`, `blocked: page_failed`, `partial[]` row `discussion_entries:topic:<id>`, exit 12 |
| A body cut at 64 KiB | `truncated: true`, `partial[]`, exit 12 |
| `--offline` with no complete coverage | exit 7 (§7) |
| Bad `--scope`, bad `--announcements`, `--page` without `--replies`, `--page 0` | exit 2 |

Auth (`401`) and throttling (`429`, or a rate-limited `403`) always propagate as exit 3 and exit 5. They are never recorded as coverage.

---

## 4. Readings chosen where the report and the brief were silent

Each takes the reading that fetches less, marks nothing, and never labels a cut or partial result complete. All nineteen are also in `docs/reads-v2.md`.

1. **The pages listing never asks for bodies.** `include[]=body` would pull one full body per page for a listing that shows titles. A body arrives only from `page`, and the per-field write rule merges it into the same row, so a later listing refresh does not drop it.
2. **`--unpublished` filters, it does not fetch.** The endpoint is the same either way. Without the flag a page Canvas reports as `published: false` is hidden; a page with no `published` field is shown, because absence is not a denial.
3. **A page is keyed by `page_id`, and the coverage scope keeps the operand the caller used** (`page:<course>:<slug>` or `page:<course>:<id>`). Canvas accepts both forms, and the two are different cache keys until a fetch says which page they name. Both write the same row.
4. **`page` needs its course operand even for a URL.** A URL naming a different course than the operand is exit 6, and so is a URL on another origin. The same rule applies to `discussion`.
5. **`syllabus` sends no request of its own.** It reads the `course` dataset. The course cache stores `syllabus_markdown` and the JSON `syllabus_refs` projection beside it, never the source HTML, so the M1-b rule that the cache holds an allowlisted projection still holds. A cache written before M8-a has no projection, and the reference lists are then empty rather than wrong.
6. **`syllabus.updated_at` is the course's own `updated_at`.** Canvas reports no revision time for a syllabus body and the course object does not carry one through the current allowlist, so the field is `null` today rather than a guess.
7. **`--announcements` takes `yes` or `no` and defaults to `yes`.** The endpoint is fixed at `only_announcements=false`, which returns every topic; `no` hides topics Canvas marks `is_announcement`. `--unread` filters the same way, on the stored `read_state` and `unread_count`. Neither flag marks anything read.
8. **Replies have their own coverage scope.** A read without `--replies` covers `topic:<id>`; with `--replies` it covers `topic:<id>:replies`. One scope can never make the other look covered. Without `--replies` the answer is `replies: []` and `replies_coverage { pages_fetched: 0, complete: false, blocked: "not_requested" }` — never `complete`.
9. **Nested replies are followed only when Canvas truncated them.** An entry carries its `recent_replies` inline; only an entry with `has_more_replies` costs a second request on `…/entries/:eid/replies`. `pages_fetched` counts every page across both routes.
10. **A reply-page failure keeps what was stored.** Fetching stops at the first failure, the pages already read are ingested, and the topic row records `replies_complete = false` with `replies_blocked`. Because the coverage lives on the row, a later cached read reports the same incompleteness and the same exit 12.
11. **The initial-post gate is a refusal, not a partial answer.** A `403` on the entries route of a topic whose `require_initial_post` is true is reported as `initial_post_required` and exit 8, and only when `--replies` was asked. A `403` on a topic without that flag is an ordinary incomplete page set. Reading the topic itself still succeeds.
12. **`--page N` selects which stored replies to show, 100 per page.** The fetch still covers the whole set, so `replies_coverage` can tell the truth. `--page` without `--replies` is exit 2, and `--page 0` is exit 2.
13. **`discussion@1` replies carry `parent_id`.** The reply list is flat and holds both top-level entries and nested replies; without `parent_id` a reader could not tell them apart.
14. **A conversation listing row is not a conversation.** `inbox show` reports `messages_complete`, false when only the listing row is cached, so an empty `messages` array is never read as "no messages".
15. **A conversation message body is plain text, bounded like every other body.** Canvas sends it as text, not HTML, so it is not converted to Markdown. Over 64 KiB it is cut, `truncated` is `true`, and the read is partial with exit 12.
16. **The unread count may be unknown.** Canvas returns it as a string. A value that is absent or unparseable stays `null` rather than becoming `0`.
17. **The unread count has its own one-row table.** The cache is per identity, so `conversation_unread` holds the single row id 1.
18. **`sync` does not refresh these datasets.** `sync` and `sync --full` keep their v1 request budgets and their §13 targets; the new reads refresh on demand.
19. **The rubric extension is additive.** A criterion keeps `id`, `description`, and `points`, and gains `long_description`, `criterion_use_range` (false when Canvas does not say), and `ratings[]` (empty when Canvas does not send one). A rubric assessment gains `rating_id`, `null` when Canvas does not name the rating. Fixtures written before M8-a still validate.

---

## 5. Behaviour changes outside the new commands

These are the places where existing behaviour moved. Each is additive or a widened rule; no existing row, key, or name was removed or renamed.

1. **Cache schema 1 → 2.** `doctor` now reports `cache schema=2`, and the "newer schema" error text moved from `user_version=101, supported=1` to `user_version=102, supported=2`. `cache stats` lists the five new tables.
2. **`syllabus_markdown` is now produced by `rich_text`, not `html_to_markdown`.** The stored Markdown therefore carries embedded placeholders and the 64 KiB bound. `courses.data_json` gained `syllabus_refs`; it still holds no raw HTML, and the M1-b assertion that proves that still passes.
3. **`assignment@1` and `submission@1` payloads gained rubric keys.** Snapshots and the registry fixtures were updated. `rating_id` is `null` when Canvas does not send one.
4. **Listing-denial retention extended.** `pages`, `discussions`, and `inbox` joined `files` and `folders` in the three places that treat `unavailable:<status>` as recorded coverage: `Dataset::ingest`, `mark_refresh_failed`, and `mark_stale_or_serve`.
5. **`refresh_listing_with_denial` is now `pub(super)`,** so a dataset module can use it. `commands::course::ensure_detail` is now `pub(crate)`, so `syllabus` can reuse it.
6. **`tests/readme.rs` parity rule generalised.** A command with subcommands but only flags of its own — `canvas inbox` — now earns a README row instead of being treated as a group. Groups with no arguments of their own (`auth`, `config`, `alias`, `cache`, `receipts`, `identity`) are unaffected; no existing row changed.
7. **`bench_fixture` relative-URL check narrowed** to a file row, because a wiki page's `url` is its slug, not a fetchable URL.
8. **`docs/bench.md` was left untouched.** `cargo xtask bench --runs 3` rewrites it; I restored the file, because the brief allows me only `docs/reads-v2.md`. The gate itself passed.

---

## 6. Gates

All six run after `git merge main`, with `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/w1`.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo nextest run --all-features` | **606 tests run, 606 passed, 0 skipped** |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | clean |
| `cargo xtask bench --runs 3` | every §13 target met |

Bench measurements, 5-course set, targets in the last column:

| Metric | Load | p50 ms | p95 ms | Target p95 | Verdict |
|---|---|---:|---:|---:|---|
| cached todo, first output | idle | 7.3 | 7.4 | 150 | ok |
| cached todo, full run | idle | 8.1 | 8.1 | 250 | ok |
| cold start | idle | 10.0 | 10.8 | 400 | ok |
| cached todo, first output | download | 6.4 | 6.4 | 150 | ok |
| cached todo, full run | download | 6.9 | 7.0 | 250 | ok |
| cold start | download | 10.2 | 10.4 | 400 | ok |

**Test count:** 606 total, up from 593 before the package. 12 come from `review_m8a.rs`, 6 from `canvas-core::markdown::extract`, and the rest are existing tests, several of whose snapshots were regenerated for the schema-version and rubric changes.

`git status --short` is empty.

---

## 7. Left for the next round

### Item 3 — M6-b is not on `main`

`git log main` shows no `merge lane/w2: M6-b`, checked before starting item 3 and again after `git merge main`. Per the brief, items 1, 2, 4–7 were done in full and the marker is `WAITING M6-b`. Not built:

- the handler extraction pattern for each of the eight new commands;
- the MCP catalog tools `pages.list`, `page.get`, `syllabus.get`, `discussions.list`, `discussion.get`, `inbox.list`, `inbox.get`, `inbox.unread_count`, all `readOnlyHint: true`;
- their entries in the catalog allowlist test and in the skill's command list.

The command modules are written as plain `run_*` functions with the projection and refresh logic already separated from rendering, so the extraction should be mechanical.

### Item 4 — M6-c is not on `main`

`git log main` shows no `merge lane/w3: M6-c`. The `inbox.unread_count` event from the `inbox_unread` dataset, under M6-c's baseline rules with the first observation silent, is left for the next round. The dataset, its `all` scope, its `conversation_unread` row, and its `refresh_inbox_unread` function are in place and already record an observation per refresh, so the event only needs the M6-c baseline hook.

Both are recorded in the "Left for the next round" section of `docs/reads-v2.md`.

---

**WAITING M6-b**
```
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/rev-m8a`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M8-a):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M8-a.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M8-a.md`
