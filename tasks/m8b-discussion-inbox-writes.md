# M8-b — Discussion and inbox writes under plans: reply, send, status, reconcile, attribution (Claude Opus, lane w1)

Post-v1 package from the agent-UX design. Read `docs/agent-ux/REPORT.md`
§3.2 (the submission tool rows as the model; the exit mappings), §3.5
(**all of it**: plan contents, states, expiry, the approval record, the
handle, the execute rules, the "Remote write" row of the tier table, the
zones paragraph: no placeholder replies, no group writes, approval to post
is not permission for AI-generated academic work), §4 (the M8-b row and
its acceptance column), `docs/agent-ux/turns/05-fable.md` item 7 and
`turns/04-*.md` on attribution, and the sources S13, S18, S22 it cites
(read the Canvas discussion topics and conversations API pages and the
pinned controllers for what a `POST` returns and what a readback shows).
Then `docs/SPEC.md` §7, §10 (mutation epochs, pending hook), §11 (upload
transport), §12.2 (journal discipline: admission, guarded transitions,
failure states, owner-absent recovery, `reconcile`, receipts), §14, §15,
§16 row 2, §19 items 15–17, Appendix D. Precedence: REPORT §3.5 defines
the plan layer and the approval binding; SPEC §12.2's journal discipline
is the model for the new operation journal and wins wherever the two
overlap. Existing code: `canvas-core::plan` (M6-a: `prepare`,
`issue_handle`, `approve`, `execute`), `canvas-core::journal`,
`canvas-core::receipts`, `canvas-api::upload` (M2-a), the M8-a datasets
and handlers (`discussions`, `discussion`, `inbox`, `inbox show`), the MCP
catalog and the elicitation round trip (M6-b), `docs/reads-v2.md`. Read
their public APIs first. Not in this package: quizzes, group writes of any
kind, marking read, editing or deleting posts, the companion.

## Contract
| Command (class D) | Request on execute | Target lock |
|---|---|---|
| `discussion reply <course> <topic\|URL> [--to ENTRY_ID] (--text TEXT \| --text-file PATH \| --text -)` | `POST /courses/:cid/discussion_topics/:tid/entries` with `message`, or `POST …/entries/:eid/replies` with `--to` | `<identity dir>/journals/topic-<tid>.lock` |
| `inbox send --to USER_ID[,…] [--subject S] (--text TEXT \| --text-file PATH \| --text -) [--attach PATH]…` | `POST /conversations` with `recipients[]`, `subject`, `body`, `group_conversation=false`, `attachment_ids[]` | `<identity dir>/journals/conversation-new-<plan-id>.lock` |
| `inbox reply <conversation_id> (--text …) [--attach PATH]…` | `POST /conversations/:id/add_message` with `body`, `attachment_ids[]` | `<identity dir>/journals/conversation-<id>.lock` |
| `operation status <journal_id>`, `operation reconcile <journal_id> [--assume-not-posted]` | `GET …/entries` (paginated) or `GET /conversations/:id?auto_mark_as_read=false` | owner lock only |

Every command runs prepare → approve → execute on `canvas-core::plan`
with a new plan `kind` (`discussion_reply`, `inbox_send`, `inbox_reply`),
the same 15-minute admission expiry, the same approval channels (`tty`,
`yes-flag`, `elicitation`), the same handle binding, and the same rule:
nothing is dispatched without a recorded approval, and one plan admits at
most one journal. The plan freezes exact recipients, thread (`topic_id`,
`parent_entry_id?` or `conversation_id`), body bytes with `input_sha256`
and `sent_sha256` (Markdown or plain text sent as Canvas expects; say
which and why), subject, and attachments (name, size, sha256, path);
execute re-hashes attachments from disk and treats changed bytes as
`invalidated` (exit 8), as M6-a does. Attachments: inbox ones go through
the §11 upload transport against `POST /users/self/files` with
`parent_folder_path` set to the conversation attachments folder, streamed
SHA-256 verified; discussion attachments are refused at prepare with
`reason: unsupported` in this package.

Refusals at prepare (exit 8, with `reason`): a group discussion (topic
`group_category_id` non-null or `group_topic_children` non-empty →
`group_write`), a locked or closed-for-comments topic (`locked`), an
initial-post gate (`initial_post_required`: never post a placeholder to
unlock a thread), a `--to` entry outside the topic (`unresolved`), an
empty body, an unknown recipient id (`unresolved`), a course or conversation
the identity cannot see (`denied`), and cross-origin URLs (exit 6).

Operation journal (migration `0004_operations`, you are the migration
owner): `operation_journal` with `journal_id`, `plan_id` (unique),
`kind`, target fields, `state` (`planned` → `posting` → `posted` |
`matched` | `outcome_unknown` | `refused` | `failed`, with `post_status`,
`response_kind`, `not_posted_evidence`), the allowlisted response record
(`id`, `created_at`, `user_id`, `message_sha256` or `body_sha256`, never
the body), `readback`, `server_match`, `attribution`, `receipt`,
`acknowledged_at`, `superseded`. Same discipline as §12.2: admission lock
before the insert, owner lock for the operation, guarded `UPDATE … WHERE
state=?` transitions, owner-absent recovery table, epochs bumped for the
touched datasets (`discussion`/`topic:<id>:replies` or `conversation`,
`inbox`, `inbox_unread`) in the success transaction, never an automatic
repost after an ambiguous timeout (the journal goes `outcome_unknown` and
only `reconcile` moves it). **Attribution** is per operation and explicit:
`accepted` (a 2xx with an object id this process observed),
`observed` (a later readback shows that id), `unproven` (readback shows a
matching digest but no id link: state `matched`), `none`. A Canvas
acceptance of a conversation is never described as delivered mail: the
human line says "accepted by Canvas", the receipt carries
`delivery: "not_observable"`.

Receipts: extend `receipts list|show|export|acknowledge` to include
operation journals (`kind` column; `receipt@1` gains `operation?` with
the fields above, additive, `null` for submissions), and the pending hook
in §10 covers operation journals for their datasets. Registry: `operation@1`
(the execute result for all three commands and for `operation status`),
`operation_reconcile@1`; `plan@1` gains the new kinds. MCP tools:
`discussion.reply.prepare|execute`, `inbox.send.prepare|execute`,
`inbox.reply.prepare|execute`, `operation.status`, `operation.reconcile`
(`assume_not_posted` defaults false), with the same annotations and the
same `input_required` approval round trip as `submission.execute`;
`replayed: true` on an executed plan (§19 item 17); add them to the
catalog allowlist test, the tool-versus-command comparison, and the skill
(a new workflow file "reply and message with approval" that carries the
course-policy boundary from REPORT §3.5 in plain words).

## Deliverables
1. Plan kinds, the migration, `canvas-core::operations` (journal, transitions,
   recovery, reconcile, attribution), the three commands, `operation
   status|reconcile`, receipts and pending-hook extensions.
2. The MCP tools and skill workflow above.
3. Fixtures: extend the wiremock and bench-5 sets with a reply-able topic,
   a locked topic, a group topic, an initial-post-gated topic, a
   conversation, a recipient, and the user-files upload endpoint; `cargo
   xtask bench --runs 3 --mcp` must still meet every target.
4. Tests (REPORT §4 M8-b acceptance, each explicit): exact recipients,
   thread, body and attachments frozen and re-verified (changed file →
   invalidated, nothing sent); no dispatch without approval on every path
   (count wiremock `POST`s); group write refused; initial-post gate never
   unlocked by a placeholder (no `POST` at all); ambiguous timeout →
   `outcome_unknown`, never resent (a second execute returns the journal);
   accepted vs observed vs unproven distinguished by readback fixtures;
   acceptance never reported as delivery; kill at every transition
   boundary and owner-absent recovery per state (helper subprocess, as
   M2-a); two racing executes → one journal; expiry checked at admission
   only; `--yes` recorded as `yes-flag`; receipts list both kinds; snapshots
   for every new schema in both modes; README/clap parity; catalog and
   skill parity.
5. `docs/writes-v2.md` (the one docs file you may write): the contract as
   built, endpoints and query strings, the journal states and recovery
   table, attribution rules, schemas, and every choice you made where the
   report and this brief were silent.

## Rules
- You own `crates/canvas-core/src/operations/**`, the migration
  `0004_operations`, the plan-kind extension, `commands/{discussion_reply,
  inbox_send,inbox_reply,operation}.rs` (or one `operation` module), the
  receipts extension, the registry entries named above, the eight MCP
  tools, the skill workflow file, `docs/writes-v2.md`, and the fixture
  additions. Shared files (command enum, registry, README, catalog, skill
  index, receipts renderer): add your entries, keep every other lane's,
  never rename or reorder.
- Work on branch `lane/w1` in the main checkout `/Users/rolfie/projects/
  canvas-cli` with `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/
  .target/w1`. Commit as you go with conventional messages. Before
  reporting, `git merge main` (resolve, rerun gates). Do not push. Do not
  merge into `main`.
- Do not touch `docs/` (except `docs/writes-v2.md`) or `tasks/`. Where
  anything is undefined, choose the reading that never sends a request
  without a recorded human approval, never resends, and never claims more
  than it observed; name each choice in `docs/writes-v2.md` and in your
  final message.

## Gates
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
cargo xtask bench --runs 3 --mcp
```
Finish with `git status --short` and reply with the marker `DONE M8-b` on
its own line.
