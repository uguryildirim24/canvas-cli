# SPEC v0.9: what changed, section by section

This is a historical record from September 10, 2026, not current setup guidance.
Use README.md and docs/SPEC.md for the frozen command surface. The task briefs
were removed. The recorded findings and checks remain.

Two consolidation passes, both in lane `w3`. Pass 1 wrote sections 20 to 23 and left
§24 and §25 as placeholders; pass 2 wrote those two from the code that
merged afterwards and re-verified the rest.

# Consolidation pass 1

2026-09-10, lane `w3`. Brief: `tasks/spec-v09-consolidation.md`.

The job was to fold what is **on `main` now** into `docs/SPEC.md`, with no new
decision. The code and its tests are the truth for "as built";
`docs/agent-ux/REPORT.md` is the truth for intent. Nothing here resolves a §19
item, and no §19 item was reworded or removed.

One line per section, so this can be checked fast.

## Added

| Section | What it now says | Source read |
|---|---|---|
| §20 Operation plans and approval | the `plans` and `approval_handles` tables, the five plan states and their guards, the 15-minute admission expiry, the approval record, the handle binding and its six refusals, what `plan_sha256` covers, the ten execute rules, the human `submit` on top, the exit mappings, and `replayed` | `canvas-core::plan`, `store/migrate.rs`, `commands/submit.rs`, review M6-a |
| §21 Agent surface | `canvas schema` and its three output forms; `canvas mcp`: two protocol revisions, the 30-tool catalog by effect, what is absent and why it is unreachable, results and `isError`, `ttlMs`/`cacheScope`, the four resources, the approval round trip, the catalog size; the skill and its five workflows; the host-matrix pointer | `mcp/*`, `output/json_schema.rs`, `commands/schema.rs`, `skill/`, reviews M6-b and M8-a2 |
| §22 Coordinator, events, `watch`, `notify` | the four shared things and their lock paths, the shared governor's two merge rules, single-flight and the 30-second waiter, foreground interest and the priority rule; the observation protocol, the baseline rules, the four shapes, the eleven kinds, the log and its retention, the cursor rules; `canvas watch` and its tick; `canvas notify`; MCP subscriptions and the invalidation map | `canvas-core::coord`, `canvas-core::events`, `canvas-api::governor`, `commands/watch.rs`, `commands/notify.rs`, `mcp/subscribe.rs`, reviews M6-c, M6-c2, M8-a3 |
| §23 Richer reads | the eight commands with their requests and datasets, the reading rules, bodies and references and the 64 KiB bound, the exit table, the rubric extension, the cache migration and config keys, the schemas | `canvas-core::sync::{pages,discussions,inbox}`, `commands/{pages,discussions,inbox}.rs`, `markdown/extract.rs`, reviews M8-a and M8-a2 |
| §24 Companion, broker, presence | one-line placeholder: M7-a and M7-b are in flight and no part of them is on `main` | `git log main`, no `extension/` directory |
| §25 Discussion and inbox writes | one-line placeholder: M8-b is in flight and no part of it is on `main` | same |

## Extended

| Section | What changed |
|---|---|
| Header | `draft v0.8` → `draft v0.9`, 2026-09-10; says which sections are the v1 contract, which are as-built, and which are placeholders |
| §5 Commands | new sub-block "Commands added after v1" with the twelve command forms; the v2-reserved line no longer reserves `inbox`, `discussions`, `discussion`, or `notify`, because they exist |
| §5 Behaviour notes | three lines pointing the new commands at §21, §22, §23 |
| §5 Command classes | `schema` → A; `notify` → B; the six M8-a reads → C; `watch` → D. `canvas mcp` gets its own sentence: it has no class, and each tool takes the class of its command |
| §7 Output contract | `schema` and `notify` join the raw-output list; a new bullet records that `watch --json` is exit 2 and that the `--jsonl` stream is a separate contract |
| §9 Config and paths | the three coordinator lock paths; a reserved row for the M7-a broker endpoint; `ttl_pages`, `ttl_discussions`, `ttl_inbox` in the `[cache]` example; a note that no `bridge.*` key exists yet and that the eight post-v1 state tables are out of `cache clear`'s reach |
| §10 Cache, state, and sync | seven new dataset rows (`pages`, `page`, `discussions`, `discussion`, `inbox`, `conversation`, `inbox_unread`); a new "Migration list" subsection with the four batches and the two `user_version` values; the one-SQLite-thread bullet now names the coordinator exception |
| §13 Architecture | one sentence naming `cargo xtask bench --watch` and `--mcp` and saying neither has a §13 target of its own |
| §14 Errors and exit codes | a "Refusal reasons" table for the four `details.reason` values on exit 8. No new exit code |
| §18 Milestones | one sentence: the round table stops at R5, and REPORT §4 holds the rounds after it |
| §19 Open questions | items 34, 35, 36 added; every existing item is verbatim |
| Appendix A | verified against the four manifests: `tokio` feature list corrected and the `net` note made true, `rmcp` feature list completed, `fs4`/`sha2`/`tracing` versions pinned, `htmd` recorded as M1-c's pick, `uuid` added, and a closing sentence listing the seven direct dependencies the table still omits |
| Appendix B | title drops "(v1)"; a second table for the nine post-v1 endpoint rows, and a line on why `/view` is not used |
| Appendix C | new subsection "What the post-v1 rounds changed": the seven merged packages, their sections, and their review files; what is still in flight; and the fate of `docs/reads-v2.md` |
| Appendix D | title drops "(v1)"; five shared objects (`Listing`, `Embedded`, `FileRef`, `ExternalLink`, `Participant`); eleven new schema rows; `error@1` notes `details.reason`; the `event@1` line document; and a table of the five additive field sets that keep their `@1` |

## §19 items added

| # | Difference |
|---|---|
| 34 | `canvas notify` has no desktop backend. REPORT §3.6 has desktop alerts consuming events; `--stdout` is the only backend, because `notify-rust`'s macOS path is Objective-C FFI and the workspace forbids `unsafe`. |
| 35 | The eight M8-a schemas have no typed arm in the schema generator, so their `canvas schema` pages describe nullable fields as non-nullable. Raised by the M8-a2 review and never recorded. |
| 36 | `canvas schema --list` derives command names from schema ids, so it names commands that do not exist, and `canvas schema "inbox show"` exits 6. Raised by the M8-a2 review and never recorded. |

No existing item was resolved, reworded, or renumbered.

## Other files

- `docs/reads-v2.md` is now a pointer at §23, §22, and §21. Its content is folded in; nothing was dropped. Only `tasks/` still links to it, and `tasks/` was not touched.

## Findings for Rolf

These are facts found while checking the code. None of them is a SPEC change.

- **`canvas watch` behaves as class D, not class C.** It opens an online session and refuses `--offline` with exit 2, which is §8's class-D rule. The module's own doc comment says class C. §5 now records the behaviour; the comment in `crates/canvas-cli/src/commands/watch.rs:1` still says class C.
- **The binary's `--help` epilogue is out of date.** `AFTER_HELP` in `crates/canvas-cli/src/cli.rs` lists the v1 commands plus `watch` and `notify`, and omits `pages`, `page`, `syllabus`, `discussions`, `discussion`, `inbox`, `schema`, and `mcp`. This pass touched no code.
- **Appendix A still omitted seven direct dependencies** before this pass: `getrandom`, `unicode-normalization`, `httpdate`, `rpassword`, `tokio-rustls`, `tempfile`, `url`. They are now named in a closing sentence rather than given rows, because they predate the post-v1 rounds.
- **`tokio`'s `net` feature is not a production feature on `main`.** It is enabled only in `canvas-api`'s dev-dependencies. The Appendix A note now says so and points at §24.

## Gates (pass 1)

```
cargo fmt --all --check      # clean
cargo nextest run --all-features
```

The test count is unchanged from the M8-a3 merge, which is the point: this pass touched no code.

---

# Consolidation pass 2

2026-09-10, lane `w3`. Brief: `tasks/spec-v09-pass2.md`.

M7-a, M7-b, and M8-b merged after pass 1. This pass wrote the two
placeholder sections from that code, re-verified the appendices, and
corrected every sentence those merges falsified. Same rules: the code on
`main` and its tests are the truth for "as built", the REPORT is the truth
for intent, no §19 item was resolved by me, and no §19 item was reworded or
removed.

## Added

| Section | What it now says | Source read |
|---|---|---|
| §24 Companion, broker, presence | the four manifest permissions and what the extension may not do; the gesture and attachment lifecycle; the five zones and the stricter-of-two rule on both ends; the one account probe and where it runs; the text release rules, the stripped parameters and the 64 KiB bound; the native host's five-step start, the ownership lock, the endpoint and its modes; `bridge-native@1` and what `Broker::update` decides; `bridge-ipc@1`, its seven operations and its thirteen reasons; the consumer trust boundary; the seven commands with their classes; the side panel and the status feed; notes and their five bounds; follow, dispatch versus load, and the two-way generation rule; panel approvals and the host's four checks; the `context.*` tools and the `/context` resource; the four schemas; and the record of what has and has not been run in a real Chrome | `extension/` (manifest and every script), `canvas-core::bridge` (`wire`, `ipc`, `note`, `framing`, `text`, `endpoint`, `state`), `canvas-cli::bridge` (`host`, `panel`, `manifest`, `client`, `release`), `commands/{bridge,here,note,open}.rs`, `mcp/{catalog,server,resources}.rs`, `output/registry.rs`, `tests/{bridge,m7b,companion}.rs`, `extension/test/`, `docs/bench.md`, reviews M7-a and M7-b |
| §25 Discussion and inbox writes | the contract table with every request and admission lock; the prepare reads and the readbacks; the three plan kinds, what they freeze, and the two body transforms; the seven prepare refusals; migration `0004_operations`, the journal states, the admission rule that differs from §12.2, the execute order, uploads, events and cache epochs; the owner-absent recovery table; the ambiguous-outcome rules and `--assume-not-posted`; the attribution ladder, `delivery`, and the response allowlist; the exit table; receipts and the pending hook; the schemas; and the eight tools plus the sixth skill workflow | `canvas-core::operations` (`record`, `prepare`, `execute`, `reconcile`, `receipt`), `canvas-core::plan`, `store/{migrate,ops}.rs`, `receipts/ops.rs`, `commands/operation.rs`, `mcp/catalog.rs`, `output/registry.rs` and its fixtures, `skill/canvas-cli/reply-and-message-with-approval.md`, review M8-b |

## Extended

| Section | What changed |
|---|---|
| Header | `draft v0.9` → `v0.9`; the placeholder sentence is replaced by a statement that no placeholder is left (see "The header" below) |
| §5 Commands | the twelve new command forms; the reserved-name line drops `bridge …`, `here`, and the write halves of `inbox` and `discussion`, which now have contracts; three new behaviour-note lines |
| §5 Command classes | `bridge *`, `note` and `open --follow` → B; `here` → C; the three writes and both operation reads → D; a closing paragraph on why `here` is C and on §19 item 37's `operation status --offline` exception |
| §7 Output contract | `bridge host` joins the raw-output list |
| §9 Config and paths | the broker endpoint row loses "reserved" and gains the Windows DACL; a new row for the broker ownership lock; the journal lock row names the three operation admission locks; a `[bridge]` block in the config example and a table of the two `bridge.*` keys; `operation_journal` joins the state tables out of `cache clear`'s reach |
| §10 Cache, state, and sync | migration `0004_operations`; `STATE_USER_VERSION` 3 → 4; a note on the `0` in `plans.course_id`/`assignment_id`; a table of the operation cache epochs; the pending hook extended with the three pending targets exactly as `pending_operations` computes them |
| §14 Errors and exit codes | the refusal table grows from four reasons to twenty-four: the thirteen companion reasons and the seven prepare refusals; a note that `zone_opaque` is exit 0; and a sentence separating M8-a's `initial_post_required:` message prefix from M8-b's `details.reason` |
| §19 Open questions | items 35 and 36 gain a "resolved by" note; items 44 and 45 added; every existing item is verbatim |
| §20 Operation plans | a paragraph naming the three M8-b plan kinds on this layer; and the correction below on what `plan_sha256` covers |
| §21.2 `canvas mcp` | 30 tools → 43, with the effect table rebuilt and counted; the four `*.execute` tools replace the single-name approval guard; the `context/{consumer_handle}` resource row says what it serves; the catalog size is re-measured from `docs/bench.md` |
| §21.3 The shipped skill | five workflows → six |
| §22.2 Events | `operation.state` and the three plan-decision events, with their datasets, scopes, dedupe keys and payloads; eleven kinds → fifteen |
| §22.4 `canvas notify` | seven groups → nine |
| §23.7 Schemas | the eight M8-a schemas now have typed arms; the "no typed arm" paragraph is replaced |
| Appendix A | every version re-verified against `Cargo.lock`, all matching; the tokio row says `net` is a production feature of `canvas-cli` since M7-a; the closing paragraph records that no post-v1 package added a crate after `rmcp` and `schemars`, and names the three crates the lock file carries twice |
| Appendix B | a table for the M8-b prepare reads, both `POST` routes per kind, the attachment upload route and the readbacks; a row saying `here` adds no endpoint; and a table for the one request the companion makes, from the browser |
| Appendix C | M7-a, M7-b and M8-b in the packages table; "still in flight" replaced by a statement that every scheduled post-v1 package is on `main`; the fate of `docs/companion.md` and `docs/writes-v2.md` |
| Appendix D | `Note`, `Follow`, `Operation`, `OperationTarget`, `OperationAttachment`, `OperationResponse`, `OperationReadback` and `OperationMatch`; rows for `here@1`, `note@1`, `follow@1`, `bridge@1`, `operation@1` and `operation_reconcile@1`; `pending` and `pending_journals` on the four §23 read schemas; the real nullability of `Journal`, `receipt@1` and `plan@1`; three more rows in the additive table |

## Where the code and a document disagreed

The code wins in each case, and each is recorded rather than quietly fixed.

- **§20's `plan_sha256` exclusion is true of a submission plan only.** §20 says the digest excludes the outbound bytes and the local file paths. `CanonicalPlan` serializes `operation` when a plan has one, and `OperationPlan` carries `body.outbound_bytes` and each `attachments[].path`, so both are inside the digest for the three M8-b kinds. §20 now says so, and §25.2 repeats it where a reader of the writes will look.
- **`docs/companion.md` printed the wrong manifest `description`.** It showed `canvas-cli browser companion broker`; `HostManifest::new` writes `canvas-cli companion broker`. §24.9 carries the string the code writes, and the template is gone from the pointer.
- **`docs/writes-v2.md` recorded a 38-tool catalog.** It was true when M8-b was written and M7-b changed it. The measured catalog is 43 tools, 344 878 bytes, about 86 235 estimated tokens, from `docs/bench.md` at commit `4a710f6`.
- **The `OperationReceipt.transform` doc comment says "`plain` or `html`".** The value written is `plain` or `text-to-html`, from `Transform::as_str`. The SPEC uses the values, not the comment.

## §19: resolved by the code

Neither item was deleted or reworded. Each keeps its original text and gains one appended line.

| # | Was | Resolved by |
|---|---|---|
| 35 | the eight M8-a schema pages describe nullable fields as non-nullable | `5c171ff`: the eight result types derive `JsonSchema` and their pages declare `result_source: "result type"` |
| 36 | `canvas schema --list` names commands that do not exist | `d18bcfa`: a registry entry carries the command that prints it; `canvas schema "inbox show"` resolves, and the five entries no command prints are listed as `document` |

Both were verified by running `canvas schema --list` and `canvas schema "inbox show"` against this build, not by reading the commits.

Items 27 and 28 stay open. Their fixes predate pass 1, which read them and deliberately left them for Rolf; nothing since has changed that.

## §19 items added

| # | Difference |
|---|---|
| 44 | The companion declares `sidePanel` as a **fourth** permission. REPORT §3.3 names two; item 30 recorded `scripting` as the third and was closed on that reading, before M7-b added `sidePanel`. It grants no host or tab access, and REPORT §3.4 forbids the alternative of a Canvas DOM overlay, so the same reasoning applies, but the confirmation on item 30 cannot cover a permission added afterwards. |
| 45 | The panel draws an operation plan without its body. `awaiting_decision` filters by plan state and handle only, so every waiting plan reaches the panel, and `PanelPlan` carries the submission half only. A discussion or inbox plan therefore draws as `assignment 0` with its kind, digests and expiry, and with no message text, no thread and no recipients, while REPORT §3.5 requires the exact bytes before an approval. Approving it there still works and every host-side check is unchanged, so it is not a forgery path. |
| 46 | The panel has no words for an operation journal's own states. `panel::journals` lists operation journals beside submission journals, and `JOURNAL_STATES` names only the ten §12.2 submission states, so `posted` and `failed` draw as states the panel does not know. Added by the review pass, not by the worker; see `docs/reviews/spec-v0.9-pass2.md`. |

No existing item was resolved by me, reworded, or renumbered. §19 now runs
1 to 46: 44 and 45 from this pass, 46 from the review of it.

## The header

"draft" is dropped. The brief's test was: drop it only if every placeholder is gone and every section describes built code.

- **Every placeholder is gone.** §24 and §25 were the only two, and both are written. A search of the document for "placeholder", "reserved for", and "in flight" now returns only ordinary prose: the reserved *command names* in §5 (`grades estimate`, `dashboard`, `submit --resume`, which are names and not sections), the Markdown placeholder line §23.3 puts in place of embedded content, the initial-post rule in §25.3, and "in flight" as a description of a request on the wire.
- **Every section describes built code.** sections 1 to 19 are the v1 contract, which shipped. Sections 20 to 25 record six merged packages, each with its own review file, and Appendix C lists all of them.

What stays open is §19, which is a list of questions for Rolf and has been part of this document since v0.1. It is not a placeholder, and it does not describe unbuilt code.

## Other files

- `docs/companion.md` is now a pointer at §24, plus two things the SPEC does not carry: the install walkthrough, and the table of Chrome checks a person runs by hand. The README links to this file for the install steps, and nothing in that package has been run in a real browser, so the table is still the only way anyone finds out whether it works.
- `docs/writes-v2.md` is now a pointer at §25, §20, §10, §9, §14, §22, §21, and Appendix D. Nothing was dropped. Only `tasks/` still links to it, and `tasks/` was not touched.

## Findings for Rolf

Facts found while checking the code. None of them is a SPEC change, and this
pass touched no code.

- **The `--help` epilogue is still headed "Commands (v1)".** It has since
  gained `discussion reply`, `inbox send|reply`, `operation status|reconcile`,
  `bridge install|host|status|detach`, `here`, and `note`, so the heading is
  now wrong about most of what it lists, and it still omits `pages`, `page`,
  `syllabus`, `discussions`, `discussion`, `inbox`, `schema`, and `mcp`. Pass
  1 raised the omission; the heading is new.
- **The panel and the writes were built by different lanes and have not met.**
  §19 item 45 is the visible seam: M7-b's panel and M8-b's plan kinds are each
  correct on their own, and no test drives one through the other. Both landed
  in the same round.
- **Nothing in `extension/` has ever run in a browser.** §24.16 says so at
  length, and `docs/companion.md` keeps the table for running the checks. The
  benchmark, the broker tests, and the npm tests all exercise code that runs
  outside Chrome or code Chrome would run, never Chrome itself.

## Gates (pass 2)

```
cargo fmt --all --check      # clean
cargo nextest run --all-features
```

923 tests, unchanged from the M7-b merge. This pass
touched no code.
