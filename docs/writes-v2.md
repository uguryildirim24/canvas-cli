# Writes v2 — discussion replies and the inbox (M8-b)

This document records the write contract as built. It is the source the SPEC
can absorb: the exact endpoints and bodies, the plan layer, the operation
journal and its recovery table, the attribution rules, the schemas, and every
choice made where `docs/agent-ux/REPORT.md` and
`tasks/m8b-discussion-inbox-writes.md` were silent.

Every command here is class D. Every one runs `prepare → approve → execute`
on `canvas-core::plan`. **Nothing is dispatched without a recorded human
approval, nothing is ever resent, and nothing claims more than it observed.**

## Contract

| Command | Request on execute | Admission lock |
|---|---|---|
| `discussion reply <course> <topic\|URL> [--to ENTRY_ID] (--text T \| --text-file P \| --text -) [--attach P]… [--yes]` | `POST /api/v1/courses/:cid/discussion_topics/:tid/entries` with `message`, or `POST …/entries/:eid/replies` with `--to` | `journals/topic-<tid>.lock` |
| `inbox send --to USER_ID[,…] [--subject S] (--text …) [--attach P]… [--yes]` | `POST /api/v1/conversations` with `recipients[]`, `subject`, `body`, `group_conversation=false`, `attachment_ids[]` | `journals/conversation-new-<plan-id>.lock` |
| `inbox reply <conversation_id> (--text …) [--attach P]… [--yes]` | `POST /api/v1/conversations/:id/add_message` with `body`, `attachment_ids[]` | `journals/conversation-<id>.lock` |
| `operation status <journal_id>` | the readback below; no write | owner lock only |
| `operation reconcile <journal_id> [--assume-not-posted]` | the readback below; no write | owner lock only |

Reads the three writes make at prepare:

| Purpose | Request |
|---|---|
| The topic, to check the gates | `GET /api/v1/courses/:cid/discussion_topics/:tid` |
| A `--to` entry, to check it is in the topic | `GET /api/v1/courses/:cid/discussion_topics/:tid/entries` |
| The conversation, for `inbox reply` | `GET /api/v1/conversations/:id?auto_mark_as_read=false` |
| Each recipient id | `GET /api/v1/search/recipients?user_id=<id>` |

The readback both `operation status` and `operation reconcile` run:

| Kind | Request |
|---|---|
| `discussion_reply` without `--to` | `GET /api/v1/courses/:cid/discussion_topics/:tid/entries` |
| `discussion_reply` with `--to ENTRY_ID` | `GET /api/v1/courses/:cid/discussion_topics/:tid/entries/:eid/replies` |
| `inbox_reply`, and `inbox_send` after Canvas named a conversation | `GET /api/v1/conversations/:id?auto_mark_as_read=false` |

A threaded reply is read on the replies route because that is where Canvas
puts it: the topic's entry listing is top-level only, so reading it would
report every threaded reply as absent.

`per_page=100` is appended by `canvas_api::Client`, so it is not written in
the source paths. `auto_mark_as_read=false` is on every conversation read, as
in M8-a: reading to check a write must never mark it read.

Attachments on an inbox write go through the §11 upload transport:
`POST /api/v1/users/self/files` with `parent_folder_path` set to
`conversation attachments` and `on_duplicate: rename`, then the returned
upload URL, with the streamed SHA-256 verified against the frozen digest.
A discussion attachment is refused at prepare with `reason: unsupported`.

## The plan

Three new plan kinds join `submission`: `discussion_reply`, `inbox_send`,
`inbox_reply`. They share the M6-a plan layer unchanged — the 15-minute
admission expiry, the approval channels `tty`, `yes-flag`, `elicitation` and
`panel`, the handle binding, the `plan_sha256` the approval is bound to, and
the rule that one plan admits at most one journal.

A plan freezes:

* the exact thread — `course_id` and `topic_id`, with `parent_entry_id` when
  `--to` was given, or `conversation_id`, or the exact recipient ids;
* the subject, for a send;
* the body, as `input_sha256` (the normalized input) and `sent_sha256` (the
  bytes that go on the wire), with the transform named;
* every attachment as name, size, SHA-256, and absolute path.

Execute re-hashes every attachment from disk. Changed bytes are `invalidated`
(exit 8) and nothing is sent, exactly as M6-a does for a submission.

Limits: 1 MiB of body text (`MAX_TEXT_BYTES`, shared with `submit`), and at
most 10 attachments.

### The body transform

`discussion_reply` sends `text-to-html`: the text is escaped and wrapped in
`<p>`/`<br>` by the same `text_to_html` a text submission uses, because
Canvas renders a discussion `message` as HTML. A `<` the person typed is
never markup.

Both inbox writes send `plain`: Canvas treats a conversation `body` as plain
text and escapes it itself, so the bytes travel as they were typed. Escaping
them here would show the person their own entities.

`input_sha256` covers the normalized input (CRLF folded to LF) so the digest
does not change with the line endings of a `--text-file`. `sent_sha256`
covers the outbound bytes, and it is the digest a readback is compared
against.

## Refusals at prepare

Every one is exit 8 with `result.details.reason`, and every one happens
before any `POST`.

| `reason` | When |
|---|---|
| `group_write` | the topic has a `group_category_id` or `group_topic_children` |
| `locked` | the topic is `locked`, `locked_for_user`, or closed for comments |
| `initial_post_required` | `require_initial_post` is set and this identity has not posted |
| `unresolved` | a `--to` entry outside the topic, or a recipient id Canvas does not return |
| `empty_body` | the body is empty or only whitespace |
| `denied` | a `403`/`401` on the course, topic, or conversation |
| `unsupported` | an attachment on a discussion reply, a body over 1 MiB, or more than 10 attachments |

A cross-origin URL is exit 6 (`resolution`), not exit 8: it is not a Canvas
object this identity was refused, it is not this identity's Canvas at all.

**The initial-post gate is never opened.** The tool does not post a
placeholder to reveal a thread, and the skill tells the host not to either.

## The operation journal

Migration `0004_operations` (state, `STATE_USER_VERSION` 4) adds
`operation_journal` and one nullable column, `plans.operation_json`.

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

### Transitions and events

Each transition writes an `operation.state` event through
`canvas-core::events` in the same transaction as the row update, with
`entity_key` the journal id, `before`/`after` carrying `{ "state": … }`, and
the scope `topic:<tid>`, `conversation:<id>`, or `conversation:new`. The
dedupe key is `operation:<journal_id>:<state>`, so a replayed execute never
writes a second event for a state the journal already reached.

### Cache epochs

A success bumps, in the same transaction:

| Kind | Scopes |
|---|---|
| `discussion_reply` | `discussion:topic:<tid>`, `discussion:topic:<tid>:replies`, `discussions:course:<cid>` |
| `inbox_send` | `inbox:*`, `inbox_unread:*` |
| `inbox_reply` | `conversation:conversation:<id>`, `inbox:*`, `inbox_unread:*` |

### Owner-absent recovery

When the owner lock is free but the journal is not terminal, the row is given
the state its interruption implies **before** anything is concluded from a
readback:

| State found | Recovered to | Why |
|---|---|---|
| `planned` | `refused`, `not_posted_evidence: never_sent` | the process died before the request was built; nothing was on the wire |
| `posting` | `outcome_unknown`, `response_kind: none` | the request may have been on the wire; the answer is not known |
| any terminal state | unchanged | there is nothing to recover |

`recover_active` runs this over every non-terminal journal of a target before
a new one is admitted, so an abandoned operation never blocks a target
forever, and never turns into a second message.

### Ambiguous outcomes

Nothing is ever resent automatically. A journal reaches `outcome_unknown`
from a timeout, a transport failure after the request was written, a crash
during `posting`, or a **5xx answer** (see decision 1). Only
`operation reconcile` moves it out, and only on evidence.

`--assume-not-posted` records that nothing was posted. It is refused while a
matching message is visible in the thread, and while the journal is younger
than 30 minutes (`ASSUME_AFTER`). The refusal is a warning on an exit-9
envelope, not a silent success.

## Attribution

Attribution is evidence, not confidence. It is per operation, and it is the
only thing a caller may repeat.

| `attribution` | State | What is true |
|---|---|---|
| `accepted` | `posted` | Canvas answered 2xx and named the object it created |
| `observed` | `posted` | a later readback shows that same object id |
| `unproven` | `matched` | the thread holds a message with the same `sent_sha256`, and nothing links it to this request |
| `none` | any other | nothing links this journal to an object in Canvas |

`delivery` is a separate field, and it is about what Canvas can report at
all: `observable` for a discussion reply, `not_observable` for both inbox
writes. **A conversation Canvas accepted is not delivered mail.** The human
line says "accepted by Canvas". The receipt carries
`delivery: "not_observable"`. Nothing in either mode prints "delivered",
"received", or "has read", and a test asserts it.

### The response record

Only an allowlist of the Canvas answer is stored. The body is never stored:

`id`, `conversation_id`, `created_at`, `created_at_local`, `user_id`,
`body_sha256` (of the body Canvas echoed), `attachment_ids`, and
`response_sha256` (of the raw HTTP response). A readback stores the same
fields plus `scanned` and `complete`.

## Exit codes

| Exit | When |
|---|---|
| 0 | `posted` or `matched` |
| 6 | an unknown journal id, a cross-origin URL, an unparseable operand |
| 8 | every prepare refusal, an invalidated plan, a spent handle, a `4xx` from Canvas (`failed`) |
| 9 | `planned`, `posting`, or `outcome_unknown` — the outcome is unknown, not failed |
| 11 | a declined or cancelled approval |

Exit 9 is `outcome: recovery`. It means "ask again", never "it failed".

## Receipts and the pending hook

`receipts list|show|export|acknowledge` cover operation journals beside
submissions. The listing gains a `kind` column and shows both;
`assignment_id` and `baseline_attempt` are `null` on an operation row.
`receipts show` and `receipts export` accept a journal id or a receipt id, and
both id spaces are searched before either is reported missing.

`receipt@1` gains `operation?`, additive and `null` for a submission. It
carries the kind, the target, the recipients and subject, the digests, the
allowlisted response, the readback, the server match, the attribution, and
the delivery field.

The §10 pending hook covers operation journals. `discussion@1`, `inbox@1`,
`conversation@1`, and `inbox_unread@1` gain `pending` and
`pending_journals[]`. A journal is pending while it is `planned` or
`posting`, and an `outcome_unknown` one is pending until it is acknowledged.
While `pending` is true, the read is not a settled picture of the thread.

## Schemas

`operation@1` is the result of all three writes and of `operation status`.
`operation_reconcile@1` is the result of `operation reconcile`. Both are in
the registry with fixtures, in the e2e shape table, and in the typed-schema
test. `plan@1` gains a nullable `operation` block and makes `course_id` and
`assignment_id` nullable, because an inbox write has neither.

Ids are strings (§7). Every field is present; unknown values are `null`; an
array is never `null`.

## The agent surface

Eight tools, each calling the same core the CLI calls:

| Tool | Command behind it |
|---|---|
| `discussion.reply.prepare` / `.execute` | `discussion reply` |
| `inbox.send.prepare` / `.execute` | `inbox send` |
| `inbox.reply.prepare` / `.execute` | `inbox reply` |
| `operation.status` | `operation status` |
| `operation.reconcile` | `operation reconcile` |

A `*.prepare` is `Organize` (it freezes a plan and posts nothing). A
`*.execute` is `RemoteWrite`. `operation.status` is a `Read`.
`operation.reconcile` is `Retire`, because `--assume-not-posted` records a
durable local decision about what did not happen.

An execute on a prepared plan returns `input_required` with a `requestState`
and one `elicitation/create` request — the same round trip
`submission.execute` uses. A host that declares no elicitation support gets a
domain refusal instead: exit 8, `reason: approval_required`, with the handle,
and **nothing is dispatched**. An execute on an already-executed plan returns
that journal's `operation@1` with `replayed: true` (§19 item 17) and creates
no second message.

There is no argument anywhere that asserts an approval.

The skill gains one workflow file, `reply-and-message-with-approval.md`,
which carries the REPORT §3.5 course-policy boundary in plain words: an
approval to post is not permission for AI-generated academic work, never
write a placeholder, never invent a recipient, show the exact text before the
approval, and never turn an acceptance into delivered mail.

`cargo xtask bench --runs 3 --mcp` still meets every target with the 38-tool
catalog.

## Decisions

Each of these is a place where the report and the brief were silent, and each
takes the reading that never sends a request without a recorded approval,
never resends, and never claims more than it observed.

1. **A 5xx is `outcome_unknown`, not `failed`.** SPEC §12.2 records that
   Canvas can answer `500` after it has committed. The answer therefore
   proves nothing about whether the write happened, and calling it `failed`
   would invite a resend. A `4xx` stays `failed`: Canvas rejected the
   request, and the journal says so. The envelope carries a warning naming
   the reason.
2. **The brief's `superseded` column is not in the table.** A submission is
   superseded by a later attempt on the same assignment; a reply or a message
   is never superseded, because a second one is a second message, with its
   own journal. Adding a column that can only ever be false would invite a
   reader to think otherwise.
3. **`operation@1` names its own plan, so `plan_id` is never null there.**
   The unique index on `plan_id` makes the link one-to-one. The registry's
   nullability test carries an explicit exception for the two schemas that
   name their own plan, rather than declaring a field nullable that cannot
   be.
4. **`inbox_send` locks on the plan id, not on a recipient set.** There is no
   conversation yet, so there is no target to lock. `conversation-new-<plan
   id>` gives each send its own admission lock, which still stops the same
   plan from being admitted twice, and does not stop a person from writing to
   two people at once.
5. **A send is pending for the whole inbox.** Until Canvas names a
   conversation, a send belongs to no conversation, so the `inbox` and
   `inbox_unread` reads report it and `inbox show` cannot. The pending query
   for a conversation therefore also matches any unresolved `inbox_send`.
6. **Expiry is checked at admission only.** A plan that expires while the
   request is on the wire does not invalidate the request; the message is
   already gone. Checking expiry later could only produce a journal that
   contradicts what Canvas holds.
7. **A digest match is `unproven`, never `observed`.** Two people can write
   the same sentence, and a person can write the same sentence twice. The
   state that carries a digest match is `matched`, and the wording says the
   thread holds a message with the same digest and nothing links it to this
   request. The candidate must also be this identity's own writing: an object
   Canvas attributes to somebody else resolves nothing. An object with no
   author at all leaves the question open, because a digest match is
   `unproven` either way.
8. **A readback that did not cover the whole thread cannot prove absence.**
   When `complete` is false, a `not_found` verdict carries a warning saying
   so, and `--assume-not-posted` is refused however old the journal is. The
   exposed case is a send Canvas never named a conversation for: there is no
   thread to read at all.
9. **`operation status` never changes state; `operation reconcile` may.**
   Both read the thread, and both record what they saw, because recording an
   observation is not a state change. Only `reconcile` moves
   `outcome_unknown` to `matched` or to an asserted "never posted", and only
   `reconcile` applies the owner-absent recovery table — SPEC §12.2 names the
   recoverers, and a readback is not one.
10. **A live owner stops recovery, not reading.** When another process holds
    the owner lock, `status` and `reconcile` still read the thread and report
    `verdict: not_read` with a warning. Changing the journal under a running
    process would be the dishonest half.
11. **`--yes` is a recorded approval, not a bypass.** It is written to the
    approval audit as channel `yes-flag`, with the same handle binding as a
    terminal confirmation. A run without `--yes` and without a controlling
    terminal is refused, not silently approved.
12. **The retry guard asks the catalog which tools ask.** M6-b bound the
    approval `requestState` to the single name `submission.execute`. With six
    more approval-gated tools, a host that answered a write approval was told
    its state belonged to another tool, and the person's decision was lost.
    The guard now admits any `*.execute` in the catalog and still rejects a
    replayed state aimed at a read.
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

## Left for the next round

* Group discussions, marking read, editing or deleting a post, and quizzes
  are out of this package by the brief.
* A discussion reply cannot carry an attachment. Canvas takes one on the
  entries route; freezing and uploading it needs the entry-scoped upload
  route, which is a package of its own. It is refused at prepare rather than
  dropped silently.
