# Review — SPEC v0.9 consolidation pass 1 (lane `w3`)

Verdict: **MERGE**, after the four `review(SPEC-v0.9):` commits below.

Reviewed 2026-09-10 in `/home/user/projects/canvas-cli/.worktrees/w3` on
branch `lane/w3`, against `git diff main...HEAD -- docs/` and the code on
`main`. `git diff main HEAD -- crates xtask skill Cargo.toml Cargo.lock` is
empty, so every "as built" claim was checked against the tree in this
worktree, which is `main`'s tree.

Brief: `tasks/spec-v09-consolidation.md`. Package report:
`tasks/review-spec-v09-pass1.md`.

The rule under review was: every normative sentence added to `docs/SPEC.md` is
true of the code on `main`, no new decision is made, and no §19 item is
resolved. Sentences were checked against modules, registry entries, fixtures,
tests and manifests — not against the REPORT or the briefs. Seven sentences
failed; all seven are corrected below. Nothing else in §§20–23, the extended
tables, or the appendices contradicted the code.

## What was corrected

| Section | What was wrong | What it says now | Evidence |
|---|---|---|---|
| §20 Plan states | "A transition that matches zero rows is a lost expected-state guard and exits 13." Only `plan::approve` does that. `journal::create_linked`'s guarded `UPDATE plans … WHERE state = 'approved'` raises `StateConflict`, which `plan::link` answers with the journal the winning execute created; `ops::expire` and `ops::invalidate` ignore a row they may not move. | Names the three readings and which transition uses which. The `WHERE … state IN (…)` phrasing also became "an expected-state guard on the row", because `approve` and the execute link guard on `state = 'approved'`, not on a set. | `crates/canvas-core/src/plan/ops.rs:237`, `:328`, `:366`; `crates/canvas-core/src/journal/ops.rs:242`; `crates/canvas-core/src/plan/execute.rs:253` |
| §20 Prepare | "runs §12.2 pre-flight step 1". `plan::prepare` runs steps 1–6. | Names the whole pre-flight, step by step. | `crates/canvas-core/src/plan/prepare.rs:51-90` |
| §20 Execute, rule 6 | "re-check steps 2 and 3". Under admission the code re-reads the plan and re-checks the expiry, the invalidation and the recorded approval. It does not re-check `plan_sha256`, the identity key, or the identity generation. | Names what is re-checked and says the digest and the identity checks are not repeated. | `crates/canvas-core/src/plan/execute.rs:120-142` |
| §22.1 Foreground interest | "`canvas submit` and `plan execute` register interest before their first pre-flight request." `submit` registers after `resolve_target`, because the interest is keyed by assignment id. §19 item 29 already records exactly this gap, so the sentence contradicted its own document. | Separates the two: `plan execute` registers before its first pre-flight request; `submit` registers as soon as the assignment id exists, and points at item 29. | `crates/canvas-cli/src/commands/submit.rs:179-200`; `crates/canvas-core/src/plan/execute.rs:100`; SPEC §19 item 29 |
| Appendix D, `discussion@1` | Sort column read "replies by `created_at` then `id`". Nothing sorts the list. | "replies in the order the reply fetch stored them: every entry page, then the nested replies, then `id`". | `crates/canvas-cli/src/commands/discussions.rs:580-585`; `crates/canvas-core/src/sync/discussions.rs:650`, `:896` |
| Appendix D, `inbox@1` | Sort column read "`last_message_at` desc". The read is `ORDER BY m.position, conversations.id` over the membership the fetch wrote. | "as Canvas returns them", which is what `discussions@1` already said. | `crates/canvas-cli/src/commands/inbox.rs:471`; `crates/canvas-core/src/sync/inbox.rs:215` |
| Appendix A, closing sentence | The list of "direct dependencies the table still omits" reads as exhaustive and omitted `rustix` =1.1.4 (`fs`, `process`), a production dependency of `canvas-cli` under `[target.'cfg(unix)'.dependencies]`. | `rustix` 1.1.4 is named, with its features and its `cfg(unix)` scope. | `crates/canvas-cli/Cargo.toml` |
| §23.1 and Appendix B | "`per_page=100` is appended by the client when a path does not already carry it" reads as every request. `Client::get` uses `api_url`; only `get_all` and `get_all_wrapped` use `api_url_with_per_page`. Both Appendix B tables were already right; the sentence was not. | "appended … to every paginated collection request whose path does not already carry it; a single-object `GET` never gets it". | `crates/canvas-api/src/lib.rs:204`, `:210-216`, `:243`, `:350-356` |
| §21.2 `cacheScope` | "Results that carry private data are `cacheScope: private`" reads as a subset. `cache_meta` writes `private` on every tool result unconditionally. | "Every result of this server is one identity's private data, so `cacheScope` is `private` on all of them." | `crates/canvas-cli/src/mcp/result.rs:128-137` |

Commits, oldest first:

| Commit | Message |
|---|---|
| `951f5a2` | `review(SPEC-v0.9): §20 and §22 say what the plan code does` |
| `7c9d8ae` | `review(SPEC-v0.9): Appendix D drops two invented sort orders` |
| `615840f` | `review(SPEC-v0.9): Appendix A names rustix; per_page is paginated-only` |
| `cddd46e` | `review(SPEC-v0.9): §21.2 says every MCP result is cacheScope private` |

No code and no test was touched. Nothing was pushed and nothing was merged
into `main`. `docs/SPEC-CHANGES-v0.9.md` needed no change: none of the nine
false or ambiguous sentences appears in it.

## What was checked and found true

### §20 Operation plans and approval

- The `plans` column list, the `(assignment_id, state)` index, the
  `approval_handles` columns and its `plan_id` index, the two journal columns
  and `submission_journal_plan … WHERE plan_id IS NOT NULL` are exactly the
  `STATE_0002` batch (`store/migrate.rs:542-585`).
- The five states and their wire names are `PlanState` (`plan/record.rs:12`).
  `expire` guards `state IN ('prepared','approved')` and `invalidate` guards
  `state IN ('prepared','approved','expired')`, which is the "reached from"
  column.
- `EXPIRY` is 15 minutes (`plan/ops.rs:18`). `guard_admission` is the only
  place expiry is evaluated, and `execute` returns an `executed` plan's
  journal before reaching it.
- The approval record's four channels, `yes-flag` for `--yes`, and the
  `channel`/`at`/`consumer?`/`plan_sha256` shape are `Approval` and
  `ApprovalChannel` (`plan/record.rs:64`, `:103`).
- The six handle refusals and their wire names are `HandleRefusal`
  (`plan/mod.rs:53-90`). The `UPDATE approval_handles SET used_at = ? WHERE
  handle = ? AND used_at IS NULL` the SPEC quotes is verbatim from
  `ops::approve`, inside the same immediate transaction that sets `approved`.
- `plan_sha256`'s field list, its two deliberate exclusions, and "object keys
  are sorted" match `CanonicalPlan` and `canonical_digest`
  (`plan/record.rs:308-390`).
- The ten frozen observations and the two sorted list fields are
  `Observations::of` (`plan/record.rs:123-190`).
- Execute rules 1–5 and 7–10 match `plan::execute` in order, including the
  5-second `CONTENTION_WAIT`, the observation comparison before
  `check_admissible`, and the guarded transaction in `create_linked` that
  inserts the journal, consumes the approval and writes the
  `submission.state` event together.
- The human `submit`: `tty` for an answered prompt, `yes-flag` for `--yes`,
  a declined prompt cancelling the plan (exit 11), an unanswerable one
  exiting 2, and `OnExisting::Refuse` giving exit 8 where
  `submission.execute` uses `OnExisting::Replay`.
- The exit-mapping table matches `map_plan_error` and `refusal_reason`; the
  `in_progress` message carries the journal id and empty `details`.
- `replayed: bool` is on `SubmitResult` and is `false` on every human path.

### §21 Agent surface

- `canvas schema` is class A and raw output; `--json` is caught in
  `Cli::validate`; an unknown operand exits 6 naming `--list`.
- The three output forms, the `refuses: ["--json"]` entry, the `line`-only
  stream-line page and its `const`-pinned `schema` field are `form_of`,
  `output_section` and `document` (`output/json_schema.rs:158-236`), with
  tests in the same file. `canvas-cli/schema@1` has no registry row, so
  `canvas schema schema` exits 6.
- `canvas mcp` binds one key and one generation, refuses to start without an
  identity with the exit-3 auth error, polls `identity.json` every two seconds
  (`IDENTITY_POLL`), and returns 13 when the binding changes.
- Both protocol revisions, the primary `2026-07-28` with `server/discover` and
  no handshake, the `2025-11-25` adapter, and the explicit refusal of anything
  else in the overridden `initialize`.
- The catalog is exactly 30 tools in the order the SPEC groups them, and every
  effect assignment in the SPEC's table matches `ToolSpec::effect`.
  `readOnlyHint` is `effect == Read`, `destructiveHint` is `Some(false)` for
  every tool. `no_tool_reaches_a_forbidden_surface` pins `submission.execute`
  as the only `RemoteWrite`.
- Every absence: 29 argument structs, all with `deny_unknown_fields`;
  `dest: None` and `force: false` in the `download.*` arm; `ics: None` and
  `alarm: None` in `calendar.list`; `open::Launch::No`; `yes: false` and the
  `text: "-"` refusal in `submit_args`; `jobs` passed through unclamped.
- `isError` for `error`, `refused`, `mismatch`, `recovery`, with `partial` a
  success; the `"type": "object"` beside the `oneOf` in
  `json_schema::document`.
- `dev.canvas-cli/ttlMs` and `dev.canvas-cli/cacheScope`, and every zero case
  the SPEC lists, including "a dataset with no TTL group" — which is what the
  eight M8-a datasets are, since `dataset_ttl` has no arm for them.
- The four resources, the `canvas://<key>/<generation>/<path>` encoding, and
  `not_attached` as a §7 refusal with `details.reason`.
- The four approval steps: a `prepared` plan mints a handle and returns
  `input_required` before any client is built; a `requestState` naming another
  tool is `invalid_params`; a host without elicitation gets `refused`, exit 8,
  `approval_required`, carrying the plan id and the handle.
- The catalog size — 30 tools, 220 855 bytes, ~55 224 tokens — is
  `docs/bench.md` line 91 verbatim, and M6-b's 22 tools / 41 891 tokens is
  `docs/reviews/code-M6-b.md:44`.
- The skill ships `SKILL.md` and exactly the five named workflows; its exit
  table includes `| 11 |`; the two catalog-diff tests exist.
- `docs/agent-hosts.md` shows both third-party hosts negotiating
  `2025-11-25`, `2026-07-28` only for the project's own clients, and the
  approval round trip listed under "What stayed untested".

### §22 Coordinator, events, `watch`, `notify`

- The four shared things and their three lock-path shapes; `create_new`,
  `fs4`, never deleted.
- Permits: `0 <= n < api_concurrency` slot files, `POLL_MIN` 2 ms,
  `POLL_MAX` 40 ms, `IO_GRACE` 5 s degrading to this process's own cap, and a
  per-process storage semaphore.
- The `governor` row's five columns and `id = 1`; `Merge::Admission` versus
  `Merge::AppliedSample`; the issue-counter realignment (`realign`); the
  silence reset owned by the row (`shared_seen_at`); the coordinator's own
  `state.sqlite` connection outside the store thread.
- `refresh_lock_name`: the `~hh` escape for every byte outside `[a-z0-9._]`,
  the 200-character `MAX_NAME`, the digest fallback.
- `REFRESH_WAIT_SECS` 30; the timed-out waiter serving
  `cache_outcome(&row, /*stale*/ true, 0, row.error)` or the exit-13 lock
  timeout; the `--fresh` rule `row.fetched_at >= now`.
- `interest(assignment_id, kind, registered_at)` with the CHECK constraint on
  `{submit, plan_execute}`; liveness by lock file; the four in-flight journal
  states; interest re-read before every refresh, not once per tick; the
  terminal `outcome_unknown` journal not being in flight.
- The observation protocol's three steps and the `<dataset>:<scope>:<rowid>:
  <fetched_at>` id; `complete = 1 AND stale = 0` as the only observable row;
  the silent first baseline; `report_gap` deleting the baseline; an `applied`
  row emitting nothing.
- The four shapes with their exact column and `data_json` allowlists, the
  `due.changed` / `grade.posted` / `grade.changed` split, and the unread
  count's unreachable `added`.
- The eleven kinds and the seven `notify` groups (`events/kind.rs`).
- `events(cursor INTEGER PRIMARY KEY AUTOINCREMENT, …)` column for column;
  `RETENTION_DAYS` 30; `expire` deleting rows in one `BEGIN IMMEDIATE`;
  `set_consumer_cursor` using `MAX(cursor, excluded.cursor)` and
  `reset_consumer_cursor` reserved for an unreplayable stored position.
- `watch`: `--offline` exit 2, `--json` exit 2 naming `--jsonl`, the tick
  order ending with `inbox_unread` last, `TICK` 30 s, `BACKOFF_MIN` 30 s to
  `BACKOFF_MAX` 900 s, `REPLAY_BATCH` 500, the whole-log replay from cursor 0,
  and `WatchResult`'s seven fields with `skipped` taking exactly
  `foreground_interest`, `journal_in_flight`, or null.
- `notify`: `open_local_session`, no client, no refresh; grouping by
  `EventKind::group`; the cursor moved only after the lines are written;
  `--since` never touching the stored position; the stderr warning and the
  stdout fallback without `--stdout`.
- Subscriptions: `META_CURSOR` is `dev.canvas-cli/cursor`; the invalidation
  map in `invalidated()` row for row; `updates()` deduplicating per batch;
  `subscribable()` excluding `Target::Context`; `served()` dropping rather
  than refusing; `Start::Resync(high_water)`; the host-named cursor never
  replacing the stored one; the shared lease and the cursor-only writes. The
  SDK's inline first request is pinned by `tests/mcp.rs:260`.

### §23 Richer reads

- All eight request paths, verbatim, including `sort=title`,
  `only_announcements=false`, `auto_mark_as_read=false` on both conversation
  routes, and `/conversations/unread_count`. `/view` appears nowhere in the
  tree. The wiremock routes in `tests/review_m8a.rs` are the same paths.
- All eight dataset/scope pairs, including `page:<course>:<operand>`,
  `topic:<id>` versus `topic:<id>:replies`, `scope:<scope>`,
  `conversation:<id>` and `all`.
- Each of the fifteen reading rules, including the `published != Some(false)`
  filter that shows a page with no `published` field, `syllabus` costing no
  request of its own and reporting the course's own `updated_at`,
  `has_more_replies` as the only second-request trigger, `REPLY_PAGE` 100,
  `replies_total` null without `--replies`, `messages_complete`, the plain-text
  conversation body, and the lenient string-or-number unread count that stays
  `null` rather than `0`.
- `BODY_LIMIT` 64 KiB cut on a character boundary; the five embedded tags and
  the five `kind` values; `reported` always `"unavailable"`; the four
  reference attributes; `is_capability_key` and `strip_userinfo` applied to the
  tree before the Markdown is rendered; the fragment / `mailto:` /
  `javascript:` drops.
- The exit table: the listing-denial `partial[]` scopes and exit 12, the
  `initial_post_required:` message prefix on an exit-8 refusal, exit 6 with
  `code: resolution` for a cross-origin or wrong-course URL, and the three
  exit-2 usage errors (`--page` without `--replies`, `--page 0`, a bad
  `--scope`). `401` and a rate-limited `403` stay fatal in
  `blocked_coverage`.
- The rubric extension field for field in `criterion_json` and the
  `rating_id` re-projection, both idempotent on rows written before M8-a.
- `0002_reads` adding all five tables and `CACHE_USER_VERSION = 2`; the three
  config keys with defaults `1h` / `15m` / `5m`, all three in `KNOWN_TOP`.
- The eight schemas registered with fixtures, and the honest
  `result_source: "registry fixture"` — `result_schema` has no typed arm for
  any of them, and `schemas/discussion.json` really does carry a string
  `message_markdown`, which is what §19 item 35 claims.

### Extended tables and appendices

- §5's twelve new command forms match the `Commands` enum and `InboxCommand`
  exactly. The class table's additions match: `schema` is identity-free,
  `notify` opens a local session, the eight reads open a session and read the
  cache, and `watch` opens an online session and refuses `--offline`.
- §7's raw-output list is `Commands::has_raw_output` exactly, and the
  `watch --json` bullet matches `Cli::validate`.
- §9's three coordinator lock paths match `permits.rs`, `single_flight.rs`
  and `interest.rs`. The eight state tables named as out of `cache clear`'s
  reach are the eight the two state migrations create.
- §10's migration list, both `user_version` constants, and the seven new
  dataset rows with their complete rules.
- §13's `--watch` and `--mcp` sentence matches `docs/bench.md` §"Agent
  surface" and the watch-tick section.
- §14's four `details.reason` values are the three `refusal_for` outputs plus
  `resources::not_attached`; no new exit code appears anywhere.
- §18's sentence is true: SPEC §18's round table ends at R5 and REPORT §4
  carries R6–R11.
- Appendix A: the `tokio` split (workspace `rt, macros, fs, time, sync`; per
  crate `io-util`, `signal`, `rt-multi-thread`, `process`) and the true `net`
  statement — `net` appears only in `canvas-api`'s `[dev-dependencies]`. The
  full `rmcp` feature set, `htmd` 0.5.5 with `markup5ever_rcdom` 0.38.0, and
  the `fs4` / `sha2` / `tracing` / `uuid` versions are the manifests'.
  "No post-v1 package added a dependency after `rmcp` and `schemars`" holds:
  the only manifest edits after `1d60945` are `rmcp`'s `local` feature,
  `tokio`'s `time` and `tracing` for `canvas-core`, and a `test-support`
  feature.
- Appendix B's nine new rows and the pre-flight `GET` that runs twice.
- Appendix C's seven packages all have the review file named, all seven exist,
  and `M8-c` / `M8-d` really are the conditional GraphQL and OAuth rows of
  REPORT §4. Nothing of M7-a, M7-b or M8-b is on `main`: no `extension/`, no
  `docs/companion.md`.
- Appendix D's five new shared objects and eleven new schema rows match the
  Rust types field for field (`output/registry.rs`), the `event@1` example
  matches `EventJson`, and no result type carries `skip_serializing_if`, so
  "every listed field is always present" still holds.
- §19: items 1–33 are byte-identical to `main`; none was resolved, reworded or
  renumbered. Items 34, 35 and 36 are each true — 34 against REPORT lines 86
  and 233 and `commands/notify.rs`, 35 against `result_schema` and the
  discussion fixture, 36 against `json_schema::command_name`, which turns
  `inbox_unread@1` into `inbox unread` and leaves `canvas schema "inbox show"`
  unregistered.
- No v1 sentence changed in meaning. The modified v1 lines are the status
  header, the v2-reserved list, four class-table rows, the raw-output bullet,
  the one-SQLite-thread bullet, the §13 targets paragraph, the Appendix A
  rows, and two appendix titles losing "(v1)".

## What I could not verify

- **The §9 broker-endpoint row.** `<data root>/bridge/<identity-key>.sock`
  (dir `0700`, socket `0600`) and the Windows named pipe are quoted correctly
  from REPORT §3.2 line 147, but nothing on `main` creates either. The row is
  labelled "reserved for M7-a, §24", so it does not claim to be built. Left as
  written; the owner may prefer §24's placeholder to hold it until M7-a
  merges, so that §9 lists only paths that exist.
- **The Appendix A tool row** (`cargo-nextest` 0.9.143, `cargo-deny` 0.20.2,
  `cargo-dist` 0.32.0, `release-plz` 0.3.164) and the toolchain line. These
  predate the pass and are not in any manifest in the tree.
- **§19 item 36's "confirmed by running the command".** The derivation is
  plain in `json_schema::command_name` and `entry_command`, and the registry
  has no entry whose command name is `inbox show`, so the claim follows from
  the code; I did not run the built binary.
- **Whether Canvas actually returns conversations newest-first.** The removed
  `inbox@1` sort claim may well be true of Canvas; it is not true of anything
  in this repository, which is why it is now stated as "as Canvas returns
  them" rather than asserted.

## Gates

Run with `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/rev-spec1`
after the four commits above. The count is the same 731 the M8-a3 merge
recorded, which is the evidence that this review touched no code.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo nextest run --all-features` | **731 tests run: 731 passed, 0 skipped** |

`git status --short` is empty.
