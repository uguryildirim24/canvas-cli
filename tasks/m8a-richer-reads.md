# M8-a — Richer reading context: pages, syllabus, rubric, discussions, inbox reads (Claude Opus, lane w1)

Post-v1 package from the agent-UX design. Read `docs/agent-ux/REPORT.md`
§3.1 (bounded projections, coverage, truncation), §3.2 (the coursework
tool rows, the exit mappings), §3.6 (the `inbox.unread_count` event kind),
§4 (the M8-a row and its acceptance column, the "Pages/inbox/discussion
reads can proceed after M6" note), §5 (the course AI-policy boundary), and
the sources S13, S18, S22 it cites (read the Canvas discussion topics,
pages, and conversations API pages, and the pinned discussion and
conversation controllers for read-state side effects). Then `docs/SPEC.md`
§5 (`course`, `assignment`, `announcement`, the reserved v2 names
`discussions`, `discussion`, `inbox`), §6 (resolution, URL forms), §7, §9
(config keys), §10 (datasets, hit predicate, field observations), §11,
§12.6 (denial handling on a batch and on a single item), §14, §15, §16,
Appendix B, Appendix D. Precedence: the REPORT's acceptance column is the
contract; SPEC §7, §10, §14 define envelopes, datasets, and exits; where
neither speaks, this brief does, and the reads-contract document in item
7 records what you decided. Existing code: `canvas-core::sync` (`refresh_*`,
datasets, TTLs), `canvas-core::store::dataset`, `canvas-core::markdown`,
`canvas-core::resolve`, the `course`, `assignment`, `announcements`,
`announcement` commands (M4-b's window datasets and denial partials are the
model for listings), `crates/canvas-cli/src/output` (registry), and, if
they are on `main`, M6-b's command handlers and MCP catalog and M6-c's
events. Read their public APIs first. Not in this package: any write
(replies, messages, marking read; M8-b), quizzes, group writes, GraphQL,
the companion.

## Contract (all commands class C; all reads; nothing is ever marked read)
| Command | Endpoint (all `per_page=100`, paginated) | Dataset / scope / TTL |
|---|---|---|
| `pages <course> [--unpublished]` | `GET /courses/:id/pages?sort=title&include[]=body` is **not** used; `GET /courses/:id/pages?sort=title` | `pages`, `course:<id>`, `ttl_pages` (default 1h) |
| `page <course> <url-slug|id|URL>` | `GET /courses/:id/pages/:url_or_id` | `page`, `page:<course>:<url>`, `ttl_pages` |
| `syllabus <course>` | none new: `courses`/`course` already carry `syllabus_body` | `courses` |
| `discussions <course> [--unread] [--announcements no]` | `GET /courses/:id/discussion_topics?only_announcements=false` | `discussions`, `course:<id>`, `ttl_discussions` (15m) |
| `discussion <course> <id|URL> [--replies] [--page N]` | `GET /courses/:id/discussion_topics/:tid`; replies via `GET …/discussion_topics/:tid/entries` and `GET …/entries/:eid/replies`, never the `/view` materialized endpoint | `discussion`, `topic:<id>`, `ttl_discussions` |
| `inbox [--scope inbox|unread|sent|archived]` | `GET /conversations?scope=…&auto_mark_as_read=false` | `inbox`, `scope:<scope>`, `ttl_inbox` (5m) |
| `inbox show <id>` | `GET /conversations/:id?auto_mark_as_read=false` | `conversation`, `conversation:<id>`, `ttl_inbox` |
| `inbox unread-count` | `GET /conversations/unread_count` | `inbox_unread`, `all`, `ttl_inbox` |
| `assignment` (extended) | existing requests; add `include[]=rubric` semantics already present in the assignment object | unchanged |

Schemas (register each in the registry with a fixture; every field present,
`null` for unknown, arrays never `null`): `pages@1`, `page@1` (`title`,
`url`, `updated_at`, `published`, `front_page`, `body_markdown?`,
`embedded: [{ kind: iframe|lti|video|audio|unknown, src_origin?, reported: unavailable }]`,
`files: [{ file_id, name?, url }]` for same-origin `/files/:id` references,
with cross-origin references listed as `external_links: [{ url }]` and
never fetched), `syllabus@1` (`course_id`, `syllabus_markdown?`, `files`,
`embedded`, `updated_at?`), `discussions@1`, `discussion@1` (topic
fields incl. `assignment_id?`, `points_possible?`, `group_category_id?`,
`group_topic_children: [{ id, group_id }]`, `require_initial_post`,
`locked`, `read_state`, `unread_count`, `message_markdown?`, `replies:
[{ id, user_id, user_name?, created_at, message_markdown?, read_state,
replies_count }]`, `replies_coverage: { pages_fetched, complete: bool }`),
`inbox@1`, `conversation@1` (subject, participants `{ id, name }`,
`workflow_state`, `last_message_at`, messages with `author_id`,
`created_at`, `body`, `attachments: [{ file_id, name, size }]`),
`inbox_unread@1` (`unread_count`). Extend `assignment@1` additively:
`rubric[]` gains `long_description?`, `criterion_use_range`, `ratings:
[{ id, description, long_description?, points }]`; `rubric_assessment[]`
gains `rating_id?`; nothing existing is renamed. Markdown conversion uses
`canvas-core::markdown`; every `<iframe>`, LTI launch, `<video>`, `<audio>`
becomes an `embedded` row and a one-line placeholder in the Markdown, so an
agent is told what it cannot see. Text bodies are bounded at 64 KiB per
document with `truncated: true` and never labelled complete when cut.

Failure rules: a listing denial (`403`, not throttle) is a `partial[]` row
exactly like M4-b's `announcements:course:<id>` rows, exit 12; a single-item
denial is exit 8 with `reason: denied`; a discussion whose
`require_initial_post` gate blocks entries returns the topic with
`replies: []`, `replies_coverage.complete = false`, and `reason:
initial_post_required` as a refused outcome (exit 8) only when `--replies`
was asked; `404` is exit 6 through the normal resolver path; a partial
replies page set (a page failed) reports `complete: false` and exit 12, and
never claims the thread is complete. Bare numeric ids for `page`,
`discussion`, and `inbox show` need the course operand where the table
says so; URL forms resolve per §6, cross-origin is exit 6. `--offline` obeys
§7 coverage. No request in this package sends a verb other than `GET` (a
test greps the wiremock request log), and no request carries
`auto_mark_as_read=true` or omits it where the table sets it.

## Deliverables
1. Datasets, refresh functions, and the config keys `ttl_pages`,
   `ttl_discussions`, `ttl_inbox` (and their `config set` allowlist
   entries, with the M4-b keys as the model).
2. The commands above with table and `--json` output, README rows, and
   `assignment`'s rubric extension.
3. If M6-b is on `main` (`git log main` shows `merge lane/w2: M6-b`): the
   handler extraction pattern for each new command and the MCP tools
   `pages.list`, `page.get`, `syllabus.get`, `discussions.list`,
   `discussion.get`, `inbox.list`, `inbox.get`, `inbox.unread_count`, all
   `readOnlyHint: true`, in the catalog allowlist test and the skill's
   command list. If M6-b is not on `main` when you reach this, do items
   1, 2, 4–7 fully, and reply `WAITING M6-b`.
4. If M6-c is on `main` (`merge lane/w3: M6-c`): produce the
   `inbox.unread_count` event from the `inbox_unread` dataset under M6-c's
   baseline rules (first observation silent). Otherwise note it as left
   for the next round.
5. Fixtures: extend the bench-5 fixture and the wiremock fixtures with
   pages (one with an iframe and a same-origin file link), a discussion
   with two reply pages and one initial-post-gated topic, a graded group
   discussion, three conversations, and the unread count; `cargo xtask
   bench --runs 3` must still meet every §13 target.
6. Tests (REPORT §4 M8-a acceptance, each explicit): schema snapshots for
   every new command in both modes; coverage/truncation honesty; listing
   denial partial and single-item denial; initial-post gate; paginated
   replies complete and incomplete; graded/group metadata present; every
   inbox request carries `auto_mark_as_read=false`; the request log shows
   only `GET`; embedded content reported; file references same-origin
   only, cross-origin listed as external and never fetched; README/clap
   parity; `assignment@1` old fixtures still validate (additive change).
7. `docs/reads-v2.md` (the one docs file you may write): the contract
   table above as built, the exact endpoints and query strings, the
   schemas, TTLs, exit rules, and every choice you made where the report
   and this brief were silent, so the SPEC can absorb it.

## Rules
- You own `crates/canvas-core/src/{pages,discussions,inbox}/**`, the new
  `refresh_*` functions and dataset rows, the new command modules, the
  registry entries named above, `docs/reads-v2.md`, and the fixture
  additions. Shared files (command enum, registry, README, the MCP catalog
  if present, `config set` allowlist): add your entries, keep every other
  lane's, never rename or reorder. No migration is expected; if you must
  add one, take the next free number and say so.
- Work on branch `lane/w1` in the main checkout `/home/user/projects/
  canvas-cli` with `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/
  .target/w1`. Commit as you go with conventional messages. Before
  reporting, `git merge main` (resolve, rerun gates). Do not push. Do not
  merge into `main`.
- Do not touch `docs/` (except `docs/reads-v2.md`) or `tasks/`. Where
  anything is undefined, choose the reading that fetches less, marks
  nothing, and never labels a cut or partial result complete; name each
  choice in `docs/reads-v2.md` and in your final message.

## Gates
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
cargo xtask bench --runs 3
```
Finish with `git status --short` and reply with the marker `DONE M8-a` on
its own line (or `WAITING M6-b` per item 3).
