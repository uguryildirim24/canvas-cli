# Reads v2 — pages, syllabus, discussions, and the inbox (M8-a)

This document records the read contract as built. It is the source the SPEC
can absorb: the exact endpoints and query strings, the datasets and their
TTLs, the schemas, the exit rules, and every choice made where
`docs/agent-ux/REPORT.md` and `tasks/m8a-richer-reads.md` were silent.

Every command here is class C. Every request is a `GET`. Nothing in this
package marks anything read.

## Contract

| Command | Request | Dataset / scope / TTL |
|---|---|---|
| `pages <course> [--unpublished]` | `GET /api/v1/courses/:id/pages?sort=title&per_page=100` | `pages` / `course:<id>` / `ttl_pages` (1h) |
| `page <course> <url-slug\|id\|URL>` | `GET /api/v1/courses/:id/pages/:url_or_id` | `page` / `page:<course>:<operand>` / `ttl_pages` |
| `syllabus <course>` | none new; the `course` dataset already carries the syllabus | `course` / `course:<id>` / `ttl_courses` |
| `discussions <course> [--unread] [--announcements yes\|no]` | `GET /api/v1/courses/:id/discussion_topics?only_announcements=false&per_page=100` | `discussions` / `course:<id>` / `ttl_discussions` (15m) |
| `discussion <course> <id\|URL> [--replies] [--page N]` | `GET /api/v1/courses/:id/discussion_topics/:tid`, and with `--replies` also `GET …/discussion_topics/:tid/entries?per_page=100` and `GET …/entries/:eid/replies?per_page=100` | `discussion` / `topic:<id>` or `topic:<id>:replies` / `ttl_discussions` |
| `inbox [--scope inbox\|unread\|sent\|archived]` | `GET /api/v1/conversations?scope=<scope>&auto_mark_as_read=false&per_page=100` | `inbox` / `scope:<scope>` / `ttl_inbox` (5m) |
| `inbox show <id>` | `GET /api/v1/conversations/:id?auto_mark_as_read=false` | `conversation` / `conversation:<id>` / `ttl_inbox` |
| `inbox unread-count` | `GET /api/v1/conversations/unread_count` | `inbox_unread` / `all` / `ttl_inbox` |
| `assignment` (extended) | unchanged requests | unchanged |

`per_page=100` is appended by `canvas_api::Client` when a path does not
already carry it, so it is not written in the source paths.

The `/view` materialized discussion endpoint is never used: it marks entries
read as a side effect of reading them.

## Config keys

`cache.ttl_pages` (default `1h`), `cache.ttl_discussions` (default `15m`),
and `cache.ttl_inbox` (default `5m`) join the M4-b keys in the `config set`
allowlist and in `[cache]` of `config.toml`.

## Cache

Cache migration `0002_reads` adds `pages`, `discussion_topics`,
`discussion_entries`, `conversations`, and `conversation_unread`, and moves
`CACHE_USER_VERSION` to 2. The brief expected no migration; these entities
have no v1 table, so 2 is the next free number.

Every table is keyed by its Canvas id, as the v1 tables are. Nested arrays —
`group_topic_children`, conversation `participants` and `messages` — live in
`data_json`, and reply entries live in `discussion_entries` with membership
under `discussion_entries` / `topic:<id>`.

## Schemas

`pages@1`, `page@1`, `syllabus@1`, `discussions@1`, `discussion@1`,
`inbox@1`, `conversation@1`, and `inbox_unread@1` are registered with
fixtures and are listed in the e2e shape table. Every field is present;
unknown values are `null`; an array is never `null`.

Ids are strings (§7). Text bodies are Markdown produced by
`canvas-core::markdown`, bounded at 64 KiB per document.

## Bodies, embedded content, and file references

`canvas-core::markdown::rich_text` converts one HTML body and returns the
Markdown plus a `BodyRefs` projection. The projection holds no HTML.

* Every `<iframe>`, `<video>`, `<audio>`, `<embed>`, and `<object>` becomes
  an `embedded` row and a one-line placeholder in the Markdown, so a reader
  is told what it cannot see. `kind` is `video`, `audio`, `lti` (an
  `<iframe>` whose source names `external_tools` or `/lti/`), `iframe`, or
  `unknown`. `reported` is always `unavailable`; nothing embedded is
  fetched.
* Every `<a href>`, `<area href>`, `<img src>`, and `<source src>` is
  resolved against the active identity origin at read time. A same-origin
  reference whose path ends in `/files/:id` becomes a `files` row; every
  other origin becomes an `external_links` row and is never fetched.
  A fragment, a `mailto:`, and a `javascript:` URL are dropped.
* A body over 64 KiB is cut on a character boundary, `truncated` is `true`,
  the envelope gains a `partial[]` row, and the exit is 12. A cut body is
  never reported as complete.

## Exits

| Case | Outcome |
|---|---|
| Listing denied (`403`/`404`, not a throttle) | `partial[]` row `pages:course:<id>`, `discussions:course:<id>`, or `inbox:scope:<scope>`, `listing.available = false`, exit 12. The denial is stored as coverage, so a later cached read reports it too. |
| Single item denied | exit 8, `code: refused` |
| Not found | exit 6 through the resolver path |
| Cross-origin URL operand | exit 6, `code: resolution` |
| `require_initial_post` gate with `--replies` | exit 8, `code: denied`, message starting `initial_post_required` |
| A reply page failed after earlier pages were stored | `replies_coverage.complete = false`, `blocked: page_failed`, `partial[]` row `discussion_entries:topic:<id>`, exit 12 |
| A body cut at 64 KiB | `truncated: true`, `partial[]`, exit 12 |
| `--offline` with no complete coverage | exit 7 (§7) |
| Bad `--scope`, bad `--announcements`, `--page` without `--replies` | exit 2 |

Auth (`401`) and throttling (`429`, or a rate-limited `403`) always
propagate as exit 3 and exit 5; they are never recorded as coverage.

## Decisions made where the report and the brief were silent

Each one takes the reading that fetches less, marks nothing, and never
calls a cut or partial result complete.

1. **The pages listing never asks for bodies.** `include[]=body` would pull
   one full body per page for a listing that shows titles. A body arrives
   only from `page`, and the per-field write rule merges it into the same
   row, so a later listing refresh does not drop it.
2. **`--unpublished` filters, it does not fetch.** The endpoint is the same
   either way. Without the flag a page Canvas reports as `published: false`
   is hidden; a page with no `published` field is shown, because absence is
   not a denial.
3. **A page is keyed by `page_id`, and the coverage scope keeps the operand
   the caller used** (`page:<course>:<slug>` or `page:<course>:<id>`).
   Canvas accepts both forms and the two are different cache keys until a
   fetch says which page they name. Both write the same row.
4. **`page` needs its course operand even for a URL.** A URL that names a
   different course than the operand is exit 6, and so is a URL on another
   origin. The same rule applies to `discussion`.
5. **`syllabus` sends no request of its own.** It reads the `course`
   dataset. The course cache stores `syllabus_markdown` and the JSON
   `syllabus_refs` projection beside it, never the source HTML, so the
   M1-b rule that the cache holds an allowlisted projection still holds. A
   cache written before M8-a has no projection, and the reference lists are
   then empty rather than wrong.
6. **`syllabus.updated_at` is the course's own `updated_at`.** Canvas
   reports no revision time for a syllabus body, and the course object does
   not carry one through the current allowlist, so the field is `null`
   today rather than a guess.
7. **`--announcements` takes `yes` or `no` and defaults to `yes`.** The
   endpoint is fixed at `only_announcements=false`, which returns every
   topic; `no` hides the topics Canvas marks `is_announcement`. `--unread`
   filters the same way, on the stored `read_state` and `unread_count`.
   Neither flag marks anything read.
8. **Replies have their own coverage scope.** A `discussion` read without
   `--replies` covers `topic:<id>`; with `--replies` it covers
   `topic:<id>:replies`. One scope can never make the other look covered.
   Without `--replies` the answer is `replies: []` and
   `replies_coverage { pages_fetched: 0, complete: false, blocked:
   "not_requested" }` — never `complete`.
9. **Nested replies are followed only when Canvas truncated them.** An
   entry carries its `recent_replies` inline; only an entry with
   `has_more_replies` costs a second request on `…/entries/:eid/replies`.
   `pages_fetched` counts every page across both routes.
10. **A reply-page failure keeps what was stored.** Fetching stops at the
    first failure, the pages already read are ingested, and the topic row
    records `replies_complete = false` with `replies_blocked`. Because the
    coverage lives on the row, a later cached read reports the same
    incompleteness and the same exit 12.
11. **The initial-post gate is a refusal, not a partial answer.** A `403`
    on the entries route of a topic whose `require_initial_post` is true is
    reported as `initial_post_required` and exit 8, and only when
    `--replies` was asked. A `403` on a topic without that flag is an
    ordinary incomplete page set. Reading the topic itself still succeeds.
12. **`--page N` selects which stored replies to show, 100 per page.** The
    fetch still covers the whole set, so `replies_coverage` can tell the
    truth. `--page` without `--replies` is exit 2, and `--page 0` is exit 2.
13. **`discussion@1` replies carry `parent_id`.** The reply list is flat and
    holds both top-level entries and nested replies; without `parent_id` a
    reader could not tell them apart.
14. **A conversation listing row is not a conversation.** `inbox show`
    reports `messages_complete`, which is false when only the listing row
    is cached, so an empty `messages` array is never read as "no messages".
15. **A conversation message body is plain text, bounded like every other
    body.** Canvas sends it as text, not HTML, so it is not converted to
    Markdown. Over 64 KiB it is cut, `truncated` is `true`, and the read is
    partial with exit 12.
16. **The unread count may be unknown.** Canvas returns it as a string. A
    value that is absent or unparseable stays `null` rather than becoming
    `0`.
17. **The unread count has its own one-row table.** The cache is per
    identity, so `conversation_unread` holds the single row id 1.
18. **`sync` does not refresh these datasets.** `sync` and `sync --full`
    keep their v1 request budgets and their §13 targets; the new reads
    refresh on demand.
19. **The rubric extension is additive.** A criterion keeps `id`,
    `description`, and `points`, and gains `long_description`,
    `criterion_use_range` (false when Canvas does not say), and `ratings[]`
    (empty when Canvas does not send one). A rubric assessment gains
    `rating_id`, `null` when Canvas does not name the rating. Fixtures
    written before M8-a still validate.

## Left for the next round

* **M6-b is not on `main`.** The handler extraction and the MCP tools
  `pages.list`, `page.get`, `syllabus.get`, `discussions.list`,
  `discussion.get`, `inbox.list`, `inbox.get`, and `inbox.unread_count`
  (all `readOnlyHint: true`) are not built, per item 3 of the brief.
* **M6-c is not on `main`.** The `inbox.unread_count` event from the
  `inbox_unread` dataset, under M6-c's baseline rules with the first
  observation silent, is left for the next round, per item 4.
