# Code review — M8-b, discussion and inbox writes (lane `lane/w1`)

## Verdict

**MERGE-AFTER-DECISION.** The package does what it is for: no path reaches a
Canvas `POST` without a consumed approval handle or a recorded `tty`/`yes-flag`
audit, a group topic and a locked topic and an initial-post gate are refused
before a journal exists and no placeholder is ever written, one plan admits
exactly one journal under a kill at every boundary, and an ambiguous outcome
becomes `outcome_unknown` and is never resent. Eleven defects were found and
fixed in eleven `review(M8-b):` commits; four of them were about the one thing
this package must never get wrong — claiming more, or less, than the evidence
carries. Nothing is pushed and nothing is merged into `main`.

The decision the owner still owes is small and does not touch the write path:
three items below, of which only "`operation status --offline`" changes a
user-visible answer, and the current answer is defensible either way.

## Gates

`CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m8b`, on the
merged tree (`git merge main` brought `tasks/review-code-m8b.md` and
`tasks/spec-v09-pass2.md` and nothing else; no conflict).

| Gate | As delivered (`28380ad`) | After the fixes (`f94fe1d`) |
|---|---|---|
| `cargo fmt --all --check` | clean | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean | clean |
| `cargo nextest run --all-features` | 788 passed, 0 skipped | **794 passed, 0 skipped** |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok | same |
| `cargo +1.88 check --workspace --all-targets` | clean | clean |
| `cargo xtask bench --runs 3 --mcp --watch` | every §13 target met | every §13 target met |

**Catalog size, confirmed: 38 tools.** The reviewed tree reports
`catalog: 38 tools, 302447 bytes, ~75625 tokens per tools/list`, against the
worker's 302 406 bytes / ~75 615 tokens. The 41-byte difference is the
`operation.status` description this review corrected. `docs/bench.md` is
re-recorded with `--watch`, so no row `main` had is dropped. Per §19 item 19
the per-tool cost keeps growing and the decision about it is still the owner's;
this package adds eight tools and changes nothing about how the document is
built.

**Dependencies.** `git diff main...HEAD -- '*Cargo.toml' Cargo.lock` is empty.
Appendix A needs no change from this lane.

## Defects found and fixed

Severity: **high** = a wrong or unproven claim about what reached Canvas, or a
path that could lead a person to write twice; **medium** = a wrong contract,
exit code, or bound; **low** = published documentation that does not match the
code.

| # | Sev | Where | What was wrong | What I changed | Commit |
|---|---|---|---|---|---|
| 1 | high | `canvas-core/src/store/ops.rs:352` `pending_operations`; `canvas-core/src/operations/ops.rs:889` `is_superseded`; `canvas-core/src/receipts/ops.rs:527` | The submission "superseded" rule was copied onto operations and keyed on `topic_id IS ? AND conversation_id IS ?`. An `inbox_send` has **no** target column until Canvas answers, so both are NULL and every accepted send retired every other unresolved send: a message whose outcome was never observed silently left the §10 pending hook (`inbox`, `inbox_unread`, `conversation`, `discussion`) and the `receipts list` column because an unrelated conversation went out afterwards. On a topic the rule is wrong in principle too — a second reply is a second post. `docs/writes-v2.md` choice 2 already said so; only the code disagreed. | An operation is never superseded. `is_superseded()` is a `const fn` returning `false`, the pending query drops the superseding sub-query, and `receipts acknowledge` / `operation reconcile` are the only things that retire an unknown outcome. `PendingTarget::Conversation` now also reports an `outcome_unknown` send, which is the send that may have landed in a conversation the journal cannot name (choice 5). | `4be94cd` |
| 2 | high | `canvas-core/src/operations/reconcile.rs:167` | `docs/writes-v2.md` choice 8 and the brief both say `--assume-not-posted` is refused while the readback is incomplete. Only the *warning* was implemented; the assertion itself went through on any readback at all. Exposed case: an `inbox_send` answered by a 500 or a timeout. Canvas named no conversation, so there is no thread to read, `complete` is `false` — and thirty minutes later the journal could be recorded `refused` / `not_posted_evidence: assumed`, "nothing was posted", on evidence that covered nothing. The person then writes again and Canvas may hold two messages. | The assertion is refused whenever `complete` is `false`, with a warning that says why; the journal stays `outcome_unknown`. | `f12e207` |
| 3 | high | `canvas-core/src/operations/reconcile.rs:296` `read_thread` | The readback read `GET …/discussion_topics/:tid/entries` for **every** discussion reply. That route is top-level only — this codebase says so itself, in `sync::discussions::discussion_entries_path` beside `discussion_replies_path` — while a `--to ENTRY_ID` reply is posted to `…/entries/:eid/replies`. So a threaded reply could never be read back: `accepted` never became `observed`, `reconcile` could never resolve an unknown threaded reply to `matched`, and the readback reported `not_found` with `complete: true` — an assertion of absence about a route that was never read. | The readback follows the parent entry's replies when the plan froze a parent entry (`prepare::entry_replies_of`), and the topic's entries otherwise. Prepare still checks a `--to` entry against the top-level listing, which is where a parent entry belongs. | `0abdd81` |
| 4 | high | `canvas-core/src/operations/reconcile.rs:340,360` | `Attribution::Unproven` documents itself as a readback showing a message "whose digest matches **and whose author is this identity**". Only the digest half was checked. A classmate who typed the same sentence, or a participant who quoted it back, moved an `outcome_unknown` journal to `matched`, outcome `ok`, exit 0, with a receipt naming their entry id. | The digest-only candidate must also be written by this identity, in both places that pick one (the readback's fallback search and `match_of`). An object Canvas names no author for still counts, because the verdict there is `unproven` either way. | `67776dd` |
| 5 | medium | `canvas-core/src/operations/reconcile.rs:140`; `canvas-cli/src/mcp/catalog.rs` | `run` applied the owner-absent recovery table for **both** commands, so `operation status` on an abandoned journal moved `planned` → `refused` or `posting` → `outcome_unknown`. SPEC §12.2 names the recoverers and says reads never lock and never transition; `docs/writes-v2.md` choice 9, the module doc, the function doc, and the MCP description ("It changes no state") all promised the same. It is a durable state change from a tool annotated `readOnlyHint: true`. | Recovery belongs to `reconcile` alone. `status` still records the readback and can move `accepted` → `observed`, which is writing down an observation; the tool description now says that instead of claiming nothing changes. | `7a6901b` |
| 6 | medium | `canvas-core/src/receipts/ops.rs:222` `export` | An operation journal with no receipt (`outcome_unknown`, `failed`, `refused`, `planned`) mapped to `ReceiptError::NotFound`, which the CLI reads as exit 13 `local`, "not found" — for a journal id it had just listed and shown. SPEC §12.2 and §14 put `export` on an ineligible journal at exit 8. `rebuild_from_journal` already returned `Refused`; only `export` disagreed. | `ReceiptError::Refused`, exit 8. | `c88ae42` |
| 7 | medium | `canvas-cli/src/commands/submit.rs:718` `agent_execute` | `operation::agent_execute` refuses a submission plan, but `submit::agent_execute` carried no mirror. An operation plan named on `submission.execute` went all the way to `canvas_core::plan::execute`, which registers foreground interest, re-reads `GET /courses/0/assignments/0` and takes the assignment admission lock before `submission_kind` refuses it — and on the way there a stale-observation comparison can invalidate an approved write plan through the wrong tool. | The guard runs first, before the replay branch and before any handle is issued: exit 8, `reason: invalidated`, nothing dispatched. | `3b67fbe` |
| 8 | medium | `canvas-core/src/operations/reconcile.rs:175` | SPEC §12.2 requires the negative assumption to state its residual risk. `--assume-not-posted` recorded `refused`/`assumed` and printed only "nothing was posted". Two things make the risk real here: the original request can still land, and a discussion reply is matched by digest while Canvas sanitizes the HTML it stores, so a body that did not match may be this journal's own reply. | The answer carries that warning beside the recorded assumption. | `d88fbc3` |
| 9 | medium | `canvas-cli/src/commands/operation.rs:988` `read_body` | Stdin was bounded at 1 MiB + 1; a `--text-file` was read with an unbounded `std::fs::read_to_string`. `frozen_body` refuses an over-long body, but only after the whole file is in memory. The submission road has read text through `read_bounded` since M1. | Both sources go through one bounded read. | `8a55789` |
| 10 | low | `canvas-cli/src/output/schemas/operation.json` | The `operation@1` fixture — what `canvas schema "operation status"` publishes and what the MCP output schema is built from — carried `"response_kind": "created"`. No code path writes that word; §12.2 defines the field as `canvas-error`, `other`, or `none`, and `commit_posted` leaves it `null` on a 2xx. | The example shows `null`. | `f16e463` |
| 11 | low | `docs/writes-v2.md` | The journal-discipline list claimed the admission lock means "one target admits one operation at a time". It does not: the lock is released once the row is published, and nothing refuses a second execute while a live journal holds the target. | The sentence says what the lock actually promises, and names the guarantee that does hold (one plan, one journal, by the plan-state guard and the unique index on `plan_id`). The behavioural question is under "Needs a decision". | `13ec971` |

Each fix carries its own regression test, and every new test fails on the code
as delivered:

* `a_later_write_never_supersedes_an_unknown_one` — two sends, the first
  unresolved, the second accepted; the first is still on every pending hook,
  and only `acknowledge` clears it.
* `an_assumption_needs_a_readback_that_covered_the_thread` — a send Canvas
  named no conversation for stays `outcome_unknown` past the thirty minutes.
* `a_threaded_reply_is_read_back_under_its_parent_entry` — the topic listing
  holds only the parent; the replies route holds the reply, and the verdict is
  `observed`.
* `a_digest_written_by_somebody_else_resolves_nothing` — same digest, another
  `user_id`: verdict `not_found`, state unchanged, no receipt.
* `status_leaves_an_abandoned_journal_where_it_found_it` — `status` reports
  `planned`; `reconcile` recovers it to `refused`/`never_sent`.
* the export refusal, inside the existing 500 test (exit 8, not 13).
* the cross-kind guard, inside the MCP write round trip, counting wiremock
  `POST`s (zero).
* `an_over_long_body_is_refused_and_never_read_whole`.
* the residual-risk warning, inside the existing `--assume-not-posted` test.

## What I checked and found sound

**Nothing is dispatched without a recorded approval.** Every refusal, decline,
cancel, expiry and invalidation path was read against the wiremock `POST`
counter. `execute` refuses an unapproved, expired, invalidated, digest-changed,
wrong-identity or wrong-generation plan before the target read; the attachment
re-hash and the target revalidation both run before `create_linked`; uploads
begin only after the transaction that consumes the approval commits. The MCP
side issues a server-minted handle, carries it in `requestState`, and spends it
inside the same immediate transaction that sets the plan `approved`; no tool
argument asserts an approval, and the catalog test forbids `yes`, `force` and
their kin on every tool. A host with no elicitation support gets exit 8
`approval_required` with nothing sent. `--yes` is written to the audit as
`yes-flag`, and a run with neither `--yes` nor a terminal is exit 2, not a
silent approval.

**The zones paragraph holds.** A group topic (`group_category_id` non-null or a
non-empty `group_topic_children`) is `group_write`; a locked or
locked-for-user topic is `locked`; a `require_initial_post` topic this identity
cannot yet see is `initial_post_required`. All three refuse at prepare, before a
journal exists, and nothing anywhere posts to open a gate. `group_conversation`
is sent `false`, always, and a unit test asserts the literal. The skill file
carries the course-policy boundary in plain words — an approval to post is not
permission for AI-generated academic work, never a placeholder, never an
invented recipient, show the text before the approval — and `tests/skill.rs`
holds the write tools to that one workflow.

**Freeze and re-verification.** The plan digest covers the whole
`OperationPlan`: target (topic and parent entry, or exact deduplicated
recipients, or conversation id), both body digests, subject, and every
attachment's name, size and `sha256`. Execute re-reads the target and re-applies
every refusal, re-hashes each attachment from disk (changed byte → `invalidated`,
no journal, no request), and the §11 upload verifies the streamed digest again.

**The ambiguous outcome.** A timeout, a transport failure, a crash in `posting`
and a 5xx all reach `outcome_unknown`, and nothing retries: the second execute
returns the same journal with one `POST` on the wire, two racing executes make
one journal, and the kill tests walk every transition boundary. A 4xx is
`failed`. Exit 9 for `planned`/`posting`/`outcome_unknown` is right, and so is
exit 8 for `failed` — Canvas declining is a refusal, and nothing was written.

**Attribution and delivery.** `accepted` needs an observed object id, `observed`
needs a readback showing that id, a digest-only match is `unproven` on state
`matched`, and `none` is the default. `delivery` is a separate axis and is
`not_observable` for both inbox writes; no rendering path prints "delivered",
"received" or "read", and a test asserts it.

**§15.** No token, no raw response body and no signed URL reaches the journal:
the response record is ids, times, an author and digests, and a unit test
asserts the raw body and the `sig=` query never survive serialization. Error
text goes through `canvas_api::redact::redact`. Receipts and exports keep the
shared `0600` writer. The new admission-lock names are validated against a
narrow alphabet, so nothing climbs out of `journals/`.

**Receipts and the pending hook.** `list`, `show`, `export` and `acknowledge`
all cover operation journals, by journal id and by receipt id, with the
submission-only fields `null` rather than invented. `discussion@1`, `inbox@1`,
`conversation@1` and `inbox_unread@1` carry `pending` and `pending_journals[]`
for the operation datasets.

**Fixtures, MSRV, deps.** The bench set gained a repliable topic, a locked
topic, a group topic, a gated topic, a conversation, a messageable recipient
and both upload routes. `cargo +1.88` is clean and the lock file is unchanged.

## Needs a decision

1. **`operation status --offline` answers from the journal instead of exit 2.**
   The brief calls the five new commands class D, and §5's class table makes a
   class-D command with `--offline` exit 2 before any I/O — which is what
   `operation reconcile` does, through `require_client`. `handle_status` instead
   returns the stored journal, on the reading that a journal is an honest answer
   about what this process observed. It is the same tension §19 item 8 already
   records for `doctor --network --offline`, where the code keeps exit 2, so the
   codebase now answers the question two ways. Either add these commands to §5's
   class table with an explicit exception, or make `status` exit 2 and point at
   `receipts show`, which is class B and already prints the journal offline.
   Current code kept.

2. **`operation.status` keeps `readOnlyHint: true` while it records a
   readback.** After fix 5 it moves no journal state, but it still writes
   `readback_json` and can move `attribution` from `accepted` to `observed`.
   That matches this catalog's established convention — every cache-backed read
   is `Effect::Read` and writes `cache.sqlite` — but §19 item 21 already asks
   the owner to confirm what the annotations describe, and this is another
   instance of the same question. Current annotation kept, description
   corrected.

3. **An operation carries no `in_progress` refusal for a live target.** §12.2
   step 2 refuses a submission while a non-terminal journal with a live owner
   holds the assignment. The operation admission lock is released once the row
   is published, so a second **separately approved** write to the same topic or
   conversation is admitted while the first is still `posting`. I believe that
   is correct — a second reply is a second post, not a replacement, and the
   guarantee that matters (one plan, one journal) is enforced by the plan-state
   guard and the unique index on `plan_id`, both of which hold under the race
   tests. But §12.2 is named as the model, so the divergence should be written
   down rather than inferred. `docs/writes-v2.md` now describes the actual
   behaviour; §25 should record the rule when this package merges.

4. **The brief's `superseded` journal field is now always `false` for an
   operation.** Fix 1 makes it so, and the worker's own choice 2 argued for it
   before the code did. The column stays because `receipts list` prints one
   table for both kinds of journal. If the owner wants a real superseding rule
   for writes, it needs a definition that is not the submission one — nothing in
   the report or the brief supplies it.

5. **A discussion reply still cannot carry an attachment** (`unsupported` at
   prepare, never dropped silently), and group writes of every kind stay
   deferred. Both are the brief's own scope, listed here only so the §25 write-up
   does not lose them.
