# Code review + fix — M8-b on branch lane/w1 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M8-b
on branch `lane/w1` (worktree `/Users/rolfie/projects/canvas-cli`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /Users/rolfie/projects/canvas-cli`. Read the package brief `tasks/m8b-discussion-inbox-writes.md` and the SPEC sections it
   cites. First run `git merge main` (resolve if needed, keep both sides' entries, regenerate snapshots rather than hand-editing, commit). Then read `git log main..HEAD --stat` and the full diff. Scope: the package is M8-b plus its item 0 (three M6-b fixups: `SchemaEntry` command names, typed schemas for the M8-a results with a fixture-validates-against-schema test, `docs/bench.md` regenerated). Contract: the brief's contract table, `docs/writes-v2.md` as built, `docs/agent-ux/REPORT.md` §3.5 (all of it, including the zones paragraph: no placeholder replies, no group writes) and §4's M8-b acceptance column, `docs/SPEC.md` §12.2 (journal discipline as the model), §14, §15, §19 items 15–17 and 27. This package is the first one that lets an agent write to other people, so attack it as such: can any path send a `POST` without a consumed approval handle or a recorded `tty`/`yes-flag` approval (count wiremock POSTs on every refusal, decline, cancel, expiry, and invalidation path)? Can a group topic, a locked topic, or an initial-post-gated topic ever receive a post, and is no placeholder ever posted to unlock a thread? Are recipients, thread, body bytes, subject, and attachments frozen at prepare and re-verified at execute (changed file → invalidated, nothing sent)? Does an ambiguous timeout or a 5xx go to `outcome_unknown` and never resend (a second execute returns the same journal; two racing executes make one journal; kill at every transition boundary recovers per the table)? Is attribution honest: `accepted` only with an observed object id, `observed` only after a readback shows it, digest-only matches `unproven`, and is a conversation acceptance never described as delivered? Do the eight new MCP tools carry the right annotations, sit in the catalog allowlist and comparison tests, and does the skill carry the course-policy boundary? Does `receipts` list, show, export and acknowledge operation journals, and does the pending hook cover their datasets? Confirm the catalog size (38 tools) and record it. Rerun `cargo xtask bench --runs 3 --mcp`. The worker's final report, for reference:

```
# M8-b — Discussion and inbox writes (lane w1) — final report

Branch `lane/w1`, main checkout `/Users/rolfie/projects/canvas-cli`,
`CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/w1`.
Head `477f7ac` (`Merge branch 'main' into lane/w1`). Nothing pushed, nothing
merged into `main`. `git status --short` is empty.

---

## 1. Item 0 — the three M6-b defects the M8-a2 reviewer could not decide

The coordinator decided to fix them; each is its own commit.

| Commit | Fix |
|---|---|
| `d18bcfa` | **(a)** `SchemaEntry` gains a `command` of its own, so `canvas schema --list` lists real commands (`inbox show`, `inbox unread-count`, `submission reconcile`, `receipts verify`) and no phantom `plan`/`reconcile`/`receipt`/`verify` rows. `canvas schema "inbox show"` resolves. Every schema id is unchanged. |
| `5c171ff` | **(b)** The eight M8-a result types get typed JSON Schemas (`JsonSchema` derive, the pattern M6-b uses for the typed arms), so a nullable field is declared nullable and a strict host validator accepts a legitimate answer. A new test validates every registry fixture against its own generated schema, so the defect class cannot return. |
| `3a23e0b` | **(c)** `docs/bench.md` regenerated with `cargo xtask bench --runs 3 --mcp` for the 30-tool catalog. |

While fixing (b) I found a second, related defect and fixed it in the same
commit: `result_schema` matched on the schema id alone, so `cache@1` —
which covers three different result shapes — advertised the `stats` shape
for `cache clear` and `cache path`. The match is now keyed on
`(id, variant)`.

---

## 2. Delivered — the M8-b package

Eight commits after item 0 (plus two merges from `main`).

| Commit | What |
|---|---|
| `775fc1a` | `feat(core)`: `canvas-core::operations` — plan kinds, migration `0004_operations`, the journal, guarded transitions, owner-absent recovery, reconcile, attribution, receipts. |
| `89f37b1` | `feat(cli)`: `discussion reply`, `inbox send`, `inbox reply`, `operation status`, `operation reconcile`, the two new registry schemas, the receipts and §10 pending-hook extensions, the eight MCP tools, README/clap parity. |
| `5d82044` | `docs(skill)`: the `reply-and-message-with-approval` workflow file. |
| `990b19d` | `test(bench)`: the fixture set gains a locked topic, a recipient search, and the user-files upload endpoints. |
| `db4dc47` | `test(core)`: 35 acceptance tests against a mock Canvas plus 3 kill/race tests driving a helper subprocess. |
| `05e8dcb` | `test(cli)`: `tests/review_m8b.rs` (10 tests, 660 lines) and the MCP write-approval round trip in `tests/mcp.rs`. |
| `cbf0f61` | `docs(writes)`: `docs/writes-v2.md`, plus the `receipts export` fix. |
| `ad0c9f4` | `docs(bench)`: the 38-tool catalog. |

**New files** (21): `crates/canvas-core/src/operations/{mod,record,prepare,
execute,reconcile,ops,receipt,tests,crash_tests}.rs`,
`crates/canvas-cli/src/commands/operation.rs`,
`crates/canvas-cli/src/output/schemas/{operation,operation_reconcile}.json`,
`crates/canvas-cli/tests/review_m8b.rs`, six snapshots,
`skill/canvas-cli/reply-and-message-with-approval.md`, `docs/writes-v2.md`.

**Diff against `main`**: 87 files, +10 347 / −280.

### Commands

| Command | Request on execute | Admission lock |
|---|---|---|
| `discussion reply <course> <topic\|URL> [--to ENTRY_ID] (--text T \| --text-file P \| --text -) [--attach P]… [--yes]` | `POST /api/v1/courses/:cid/discussion_topics/:tid/entries` with `message`, or `POST …/entries/:eid/replies` with `--to` | `journals/topic-<tid>.lock` |
| `inbox send --to USER_ID[,…] [--subject S] (--text …) [--attach P]… [--yes]` | `POST /api/v1/conversations` with `recipients[]`, `subject`, `body`, `group_conversation=false`, `attachment_ids[]` | `journals/conversation-new-<plan-id>.lock` |
| `inbox reply <conversation_id> (--text …) [--attach P]… [--yes]` | `POST /api/v1/conversations/:id/add_message` with `body`, `attachment_ids[]` | `journals/conversation-<id>.lock` |
| `operation status <journal_id>` | readback only; no write | owner lock only |
| `operation reconcile <journal_id> [--assume-not-posted]` | readback only; no write | owner lock only |

Reads at prepare: the topic
(`GET /api/v1/courses/:cid/discussion_topics/:tid`), the entries when `--to`
is given, the conversation
(`GET /api/v1/conversations/:id?auto_mark_as_read=false`), and each
recipient (`GET /api/v1/search/recipients?user_id=<id>`). Inbox attachments
go through the §11 upload transport against `POST /api/v1/users/self/files`
with `parent_folder_path: "conversation attachments"` and
`on_duplicate: rename`, streamed SHA-256 verified. A discussion attachment
is refused at prepare with `reason: unsupported`.

### Refusals at prepare (exit 8, `result.details.reason`)

`group_write`, `locked`, `initial_post_required`, `unresolved`,
`empty_body`, `denied`, `unsupported`. A cross-origin URL is exit 6
(`resolution`) — it is not this identity's Canvas at all.

**The initial-post gate is never opened.** No placeholder is ever posted,
and the skill tells the host not to post one either.

---

## 3. The journal and attribution design

### Migration `0004_operations` (state, `STATE_USER_VERSION` 4)

Adds `operation_journal` and one nullable column, `plans.operation_json`.
Migrations `0001`, `0002`, `0003` are other lanes'; this one is 4, as the
coordinator noted.

States: `planned → posting → posted | matched | outcome_unknown | refused |
failed`.

The discipline is SPEC §12.2's, with the submission target replaced by an
operation target:

* the admission lock is held before the insert, so one target admits one
  operation at a time;
* an owner lock is held for the whole operation;
* every transition is a guarded `UPDATE … WHERE state = ?`, so a lost race
  changes nothing;
* the row update, the `operation.state` event, the receipt, and the cache
  epochs commit in **one transaction**.

### Events (through `canvas-core::events`, as the coordinator asked)

Each transition writes an `operation.state` event in the same transaction as
the row update: `entity_key` is the journal id, `before`/`after` carry
`{ "state": … }`, the scope is `topic:<tid>`, `conversation:<id>`, or
`conversation:new`, and the dedupe key is
`operation:<journal_id>:<state>` — so a replayed execute never writes a
second event for a state the journal already reached.
`EventKind::OperationState` is new. `mcp/subscribe.rs` maps
`operation_journal` to the `receipts` resource.

### Cache epochs bumped on success

| Kind | Scopes |
|---|---|
| `discussion_reply` | `discussion:topic:<tid>`, `discussion:topic:<tid>:replies`, `discussions:course:<cid>` |
| `inbox_send` | `inbox:*`, `inbox_unread:*` |
| `inbox_reply` | `conversation:conversation:<id>`, `inbox:*`, `inbox_unread:*` |

### Owner-absent recovery

| State found | Recovered to | Why |
|---|---|---|
| `planned` | `refused`, `not_posted_evidence: never_sent` | the process died before the request was built; nothing was on the wire |
| `posting` | `outcome_unknown`, `response_kind: none` | the request may have been on the wire; the answer is not known |
| any terminal state | unchanged | there is nothing to recover |

`recover_active` runs this over every non-terminal journal of a target
before a new one is admitted, so an abandoned operation never blocks a
target forever and never becomes a second message.

### Attribution

| `attribution` | State | What is true |
|---|---|---|
| `accepted` | `posted` | Canvas answered 2xx and named the object it created |
| `observed` | `posted` | a later readback shows that same object id |
| `unproven` | `matched` | the thread holds a message with the same `sent_sha256`, and nothing links it to this request |
| `none` | any other | nothing links this journal to an object in Canvas |

`delivery` is a **separate** field about what Canvas can report at all:
`observable` for a discussion reply, `not_observable` for both inbox writes.
A conversation Canvas accepted is never called delivered mail. Neither mode
prints "delivered", "was received", or "has read", and a test asserts it.

### Body transform

`discussion_reply` sends `text-to-html` (the same `text_to_html` a text
submission uses), because Canvas renders a discussion `message` as HTML — a
`<` a person typed is never markup. Both inbox writes send `plain`, because
Canvas treats a conversation `body` as plain text and escapes it itself.
`input_sha256` covers the normalized input (CRLF folded to LF);
`sent_sha256` covers the outbound bytes and is what a readback is compared
against.

### Ambiguous outcomes

Nothing is ever resent automatically. `outcome_unknown` comes from a
timeout, a transport failure after the request was written, a crash during
`posting`, or a 5xx (see choice 1). Only `operation reconcile` moves it, and
only on evidence. `--assume-not-posted` is refused while a matching message
is visible and while the journal is younger than 30 minutes (`ASSUME_AFTER`).

### Exit codes

| Exit | When |
|---|---|
| 0 | `posted` or `matched` |
| 6 | unknown journal id, cross-origin URL, unparseable operand |
| 8 | every prepare refusal, an invalidated plan, a spent handle, a 4xx (`failed`) |
| 9 | `planned`, `posting`, `outcome_unknown` — outcome unknown, not failed (`outcome: recovery`) |
| 11 | a declined or cancelled approval |

### Receipts and the §10 pending hook

`receipts list|show|export|acknowledge` cover operation journals beside
submissions. The listing gains a `kind` column; `assignment_id` and
`baseline_attempt` are `null` on an operation row. `receipt@1` gains
`operation?` (additive, `null` for a submission) carrying the kind, target,
recipients, subject, digests, allowlisted response, readback, server match,
attribution, and delivery. `discussion@1`, `inbox@1`, `conversation@1`, and
`inbox_unread@1` gain `pending` and `pending_journals[]`.

### Schemas

`operation@1` (the result of all three writes and of `operation status`) and
`operation_reconcile@1`, both with fixtures, in the e2e shape table and the
typed-schema test. `plan@1` gains a nullable `operation` block and nullable
`course_id`/`assignment_id`, because an inbox write has neither.

---

## 4. The MCP tools and the skill

| Tool | Effect |
|---|---|
| `discussion.reply.prepare` | `Organize` |
| `discussion.reply.execute` | `RemoteWrite` |
| `inbox.send.prepare` | `Organize` |
| `inbox.send.execute` | `RemoteWrite` |
| `inbox.reply.prepare` | `Organize` |
| `inbox.reply.execute` | `RemoteWrite` |
| `operation.status` | `Read` |
| `operation.reconcile` | `Retire` (`--assume-not-posted` records a durable local decision) |

An execute on a prepared plan returns `input_required` with a `requestState`
and one `elicitation/create` request — the same round trip
`submission.execute` uses. A host that declares no elicitation support gets
a domain refusal instead: exit 8, `reason: approval_required`, with the
handle, and **nothing is dispatched**. An execute on an already-executed
plan returns that journal's `operation@1` with `replayed: true` (§19 item
17) and creates no second message. No argument anywhere asserts an approval.

All eight are in the catalog allowlist test, the tool-versus-command
comparison, and the skill's command list.

**Skill**: `skill/canvas-cli/reply-and-message-with-approval.md` carries the
REPORT §3.5 course-policy boundary in plain words — an approval to post is
not permission for AI-generated academic work, never write a placeholder,
never invent a recipient, show the exact text before the approval, and never
turn an acceptance into delivered mail. `tests/skill.rs` gains
`the_write_tools_are_only_named_by_the_reply_workflow`;
`xtask/src/bench_mcp.rs` gains the sixth workflow and
`the_six_skill_workflows_are_covered`.

---

## 5. Defects found on the way

Both were found by the new tests, and both are fixed on the branch.

1. **The MCP retry guard admitted only `submission.execute`.** M6-b bound
   the approval `requestState` to that one name. With six more
   approval-gated tools, a host that answered a *write* approval was told
   its state belonged to another tool, and the person's decision was lost —
   the approval could not be recorded at all through MCP. The guard now asks
   the catalog (`catalog::asks_for_approval`, any `*.execute`) and still
   rejects a replayed state aimed at a read. A test covers both halves.
2. **`receipts export` could not find an operation journal by its own id.**
   `resolve_journal_id` searched only `submission_journal`, so an operation
   receipt could be shown but not exported. `export` now resolves the
   operation id space first, as `show` already did. Covered by a test in
   `review_m8b.rs`.

A third, in item 0, is listed there: `result_schema` keyed on the schema id
alone, so `cache@1` advertised the `stats` shape for `cache clear` and
`cache path`.

---

## 6. The fifteen choices

Where the report and the brief were silent, each takes the reading that
never sends a request without a recorded approval, never resends, and never
claims more than it observed. All fifteen are also in `docs/writes-v2.md`.

1. **A 5xx is `outcome_unknown`, not `failed`.** SPEC §12.2 records that
   Canvas can answer 500 after it has committed. The answer proves nothing
   about whether the write happened, and calling it `failed` would invite a
   resend. A 4xx stays `failed`. The envelope carries a warning naming the
   reason.
2. **The brief's `superseded` column is not in the table.** A submission is
   superseded by a later attempt on the same assignment; a reply or a
   message is never superseded, because a second one is a second message
   with its own journal. A column that could only ever be false would invite
   a reader to think otherwise.
3. **`operation@1` names its own plan, so `plan_id` is never null there.**
   The unique index on `plan_id` makes the link one-to-one. The registry's
   nullability test carries an explicit exception for the two schemas that
   name their own plan, rather than declaring a field nullable that cannot
   be.
4. **`inbox_send` locks on the plan id, not on a recipient set.** There is
   no conversation yet, so there is no target to lock.
   `conversation-new-<plan-id>` still stops the same plan being admitted
   twice, and does not stop a person writing to two people at once.
5. **A send is pending for the whole inbox.** Until Canvas names a
   conversation, a send belongs to no conversation, so `inbox` and
   `inbox_unread` report it and `inbox show` cannot. The pending query for a
   conversation therefore also matches any unresolved `inbox_send`.
6. **Expiry is checked at admission only.** A plan that expires while the
   request is on the wire does not invalidate the request; the message is
   already gone. Checking later could only produce a journal that
   contradicts what Canvas holds.
7. **A digest match is `unproven`, never `observed`.** Two people can write
   the same sentence, and a person can write the same sentence twice. The
   state that carries a digest match is `matched`, and the wording says the
   thread holds a message with the same digest and nothing links it to this
   request.
8. **A readback that did not cover the whole thread cannot prove absence.**
   When `complete` is false, a `not_found` verdict carries a warning saying
   so, and `--assume-not-posted` is still refused.
9. **`operation status` never changes state; `operation reconcile` may.**
   Both read the thread and both record what they saw, because recording an
   observation is not a state change. Only `reconcile` moves
   `outcome_unknown` to `matched` or to an asserted "never posted".
10. **A live owner stops recovery, not reading.** When another process holds
    the owner lock, `status` and `reconcile` still read the thread and
    report `verdict: not_read` with a warning. Changing the journal under a
    running process would be the dishonest half.
11. **`--yes` is a recorded approval, not a bypass.** It is written to the
    approval audit as channel `yes-flag`, with the same handle binding as a
    terminal confirmation. A run without `--yes` and without a controlling
    terminal is refused, not silently approved.
12. **The retry guard asks the catalog which tools ask.** See defect 1
    above. The guard now admits any `*.execute` in the catalog and still
    rejects a replayed state aimed at a read.
13. **`inbox send` never sets `group_conversation`.** It is sent as `false`,
    always. Group writes are out of this package, and a bulk private message
    to several people is what the brief asks for.
14. **An upload failure is `refused`, not `failed`.** Nothing reached the
    conversation endpoint, so nothing was posted. The journal records
    `never_sent` and names the attachment that stopped it.
15. **The three write commands share one module.** `commands/operation.rs`
    holds all three writes and both read-backs, because they differ in what
    they freeze, not in how they are approved. The brief allowed either
    shape.

---

## 7. Catalog size and the bench

`cargo xtask bench --runs 3 --mcp`, every §13 target met:

| Metric | Load | p50 ms | p95 ms | Target p95 | Verdict |
|---|---|---:|---:|---:|---|
| cached todo, first output | idle | 7.0 | 7.2 | 150 | ok |
| cached todo, full run | idle | 7.7 | 7.8 | 250 | ok |
| cold start | idle | 10.1 | 10.7 | 400 | ok |
| cached todo, first output | download | 6.9 | 6.9 | 150 | ok |
| cached todo, full run | download | 7.4 | 7.5 | 250 | ok |
| cold start | download | 11.2 | 11.5 | 400 | ok |
| cached todo, first output | watch | 7.1 | 7.2 | 150 | ok |
| cached todo, full run | watch | 7.7 | 7.8 | 250 | ok |
| cold start | watch | 11.3 | 11.4 | 400 | ok |
| watch tick | idle | 21.5 | 22.1 | none | — |
| warm `todo.list` over stdio | mcp | 2.5 | 2.6 | 100 | ok |

**Catalog: 38 tools, 302 406 bytes, about 75 615 estimated tokens per
`tools/list`** — up from 30 tools and about 55 200 tokens after M8-a. The
growth is the per-tool cost SPEC §19 item 19 already tracks: every tool
inlines the whole §7 envelope in both shapes. Nothing in this package
changes how the document is built. The decision about that cost is still the
owner's.

The recorded run includes the `watch` load `main` added, so regenerating
`docs/bench.md` for the gate does not drop a row `main` had.

---

## 8. Dependencies for Appendix A

**No new dependencies.** `git diff main...HEAD -- '*Cargo.toml' Cargo.lock`
is empty. Everything in this package is built on crates already in the
workspace: `rusqlite`, `serde`/`serde_json`, `jiff`, `sha2`, `uuid`,
`schemars`, `rmcp`, and, in tests only, `wiremock`, `insta`, and
`tempfile`. Appendix A needs no change from lane w1.

---

## 9. Gates

All run on the merged tree (`477f7ac`), after `git merge main`.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | ok |
| `cargo clippy --all-targets --all-features -- -D warnings` | ok |
| `cargo nextest run --all-features` | **788 tests run: 788 passed, 0 skipped** |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | ok |
| `cargo xtask bench --runs 3 --mcp` | every target ok; 38 tools |

### Test count by area added in this package

* `canvas-core::operations::tests` — 35 acceptance tests against a mock
  Canvas: plan freezing (thread, body, digests, labels), recipient dedup, a
  changed attachment byte → `invalidated` with no journal, all seven prepare
  refusals, no dispatch without approval (counting POSTs), `--yes` recorded
  as `yes-flag`, accepted vs observed vs unproven, acceptance never reported
  as delivery, non-2xx → `failed`, a second execute returning the same
  journal with one POST, expiry at admission only, an ambiguous 500 →
  `outcome_unknown` never resent plus the pending hook,
  `--assume-not-posted` refused early and recorded late, the owner-absent
  recovery table, and a live owner stopping recovery.
* `canvas-core::operations::crash_tests::kills` — 3 tests driving a helper
  subprocess: a kill at every transition boundary recovers to the state it
  implies (`owner_acquired`/`inserted` → no journal; `published` →
  `Refused`/`NeverSent`; `posting`/`response_received` → `OutcomeUnknown`),
  and two racing executes produce one journal and exactly one POST.
* `canvas-lms-cli::review_m8b` — 10 command-surface tests (660 lines): a
  reply posts once and lists beside a submission receipt (with `show` by
  both id spaces and `export`), a conversation Canvas accepted is never
  called delivered, each topic refusal is exit 8 with its reason and no
  POST, an unknown recipient and a discussion attachment are refused, a
  cross-origin URL and an unknown journal are exit 6, a 500 stays unknown
  and reconcile resolves it without resending (with `--assume-not-posted`
  refused early), a digest match reads back as `unproven`, both new schemas
  snapshotted in JSON and table mode, and `canvas schema` naming both new
  commands.
* `canvas-lms-cli::mcp` — the write approval round trip: prepare freezes and
  sends nothing, the ask sends nothing, the accepted approval sends once,
  the second execute replays, `operation.status` sends nothing, the receipt
  names channel `elicitation` and the host, and a replayed state aimed at a
  read is rejected.

---

## 10. Left for the next round

* Group discussions, marking read, editing or deleting a post, and quizzes
  are out of this package by the brief.
* A discussion reply cannot carry an attachment. Canvas takes one on the
  entries route, but freezing and uploading it needs the entry-scoped upload
  route, which is a package of its own. It is refused at prepare rather than
  dropped silently.
* The `tools/list` document size (§19 item 19) keeps growing per tool. That
  decision is the owner's, not this lane's.
```
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m8b`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M8-b):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M8-b.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M8-b.md`
