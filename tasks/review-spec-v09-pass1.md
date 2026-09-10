# Review — SPEC v0.9 consolidation pass 1, branch `lane/w3` (Claude Opus 5 high)

You are the reviewer of a **documentation** package. Worktree
`/Users/rolfie/projects/canvas-cli/.worktrees/w3`, branch `lane/w3`, package
brief `tasks/spec-v09-consolidation.md`. Use
`CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-spec1`.

First run `git merge main` (expect nothing to do). Then read
`docs/SPEC-CHANGES-v0.9.md` and `git diff main...HEAD -- docs/` in full.

The package's only rule was: every normative sentence added to `docs/SPEC.md`
must be true of the code on `main`, no new decision may be made, and no §19
item may be resolved. Your job is to check that, sentence by sentence, against
the code and its tests — not against the REPORT or the briefs. For every new
section (§20–§23), every extended table (§5, §7, §9, §10, §14, migrations,
Appendix A, B, D), and Appendix C: open the module, the registry entry, the
fixture, the test, or the manifest that proves the statement. Pay special
attention to: exit codes and `reason` values; every `@1` payload field and
whether it is really always present and nullable where written; every endpoint
and query string in Appendix B against the request builders and the wiremock
routes; every TTL and config key against the config defaults and the
`config set` allowlist; the migration numbers and `user_version` values; the
tool catalog names and annotations against the MCP catalog test; Appendix A
against `Cargo.toml` and `Cargo.lock` (versions, features, dev-only versus
production); and that no existing v1 sentence was changed in meaning.

Fix every false, invented, or ambiguous sentence yourself in separate
`review(SPEC-v0.9):` commits — by correcting it to what the code does, or by
deleting it when the code proves nothing either way. If the code is wrong and
the sentence is right by the REPORT, do **not** change the code: leave the
sentence describing the code and add a §19 item naming the gap. Never push,
never merge into `main`, touch no code and no tests.

Write `docs/reviews/spec-v0.9-pass1.md`: the verdict (MERGE or REWORK), a table
of what you corrected (section, what was wrong, what it says now, the evidence
file), what you checked and found true, and what you could not verify. Run
`cargo fmt --all --check` and `cargo nextest run --all-features` at the end to
prove no code changed (expect 731 passed). Finish with `git status --short` and
reply exactly on its own line: `DONE docs/reviews/spec-v0.9-pass1.md`

The worker's final report, for reference:

```
# SPEC v0.9 consolidation, pass 1 — final report (lane `w3`)

Worktree `/Users/rolfie/projects/canvas-cli/.worktrees/w3`, branch `lane/w3`,
`CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/w3`.
Brief: `tasks/spec-v09-consolidation.md`.

`docs/SPEC.md` is now **v0.9 (draft)**, dated 2026-09-10. Eleven `docs(spec):`
commits. Three files changed. No code, no tests, no other `docs/`, no `tasks/`.
Nothing was pushed and nothing was merged into `main`.

| File | Change |
|---|---|
| `docs/SPEC.md` | +606 / −13 |
| `docs/SPEC-CHANGES-v0.9.md` | new, 74 lines |
| `docs/reads-v2.md` | folded into §23 and reduced to a pointer (−315) |

## Commits

| Commit | Message |
|---|---|
| `2e15d4c` | `docs(spec): §20 operation plans and approval, as M6-a built it` |
| `9d006e5` | `docs(spec): §21 agent surface, as M6-b and M8-a2 built it` |
| `cdd74ab` | `docs(spec): §22 coordinator, events, watch and notify, as M6-c built them` |
| `8beb2e9` | `docs(spec): §23 richer reads, plus placeholders for §24 and §25` |
| `869bc00` | `docs(spec): §19 items 34-36 from the consolidation pass` |
| `a76e7ac` | `docs(spec): extend §5, §7, §9, §10 and §14 with the post-v1 surfaces` |
| `abea039` | `docs(spec): Appendix A verified against the workspace manifests` |
| `29eb793` | `docs(spec): Appendix B and Appendix D for the post-v1 payloads` |
| `4fa5a80` | `docs(spec): bump to v0.9 (draft) and record the post-v1 rounds in Appendix C` |
| `5d5450b` | `docs(spec): reduce docs/reads-v2.md to a pointer at SPEC §23` |
| `45ede3c` | `docs(spec): SPEC-CHANGES-v0.9.md, one line per section` |

## Method

The brief's precedence rule was followed literally: the code on `main` and its
tests are the truth for "as built", `docs/agent-ux/REPORT.md` is the truth for
intent. Every normative sentence was written from a file in the tree, not from
a review or a brief. Where a review and the code disagreed, the code won and
the difference became a §19 item or an entry in `SPEC-CHANGES-v0.9.md`.

Read before writing: all of `docs/SPEC.md`; `REPORT.md` §3–§4;
`docs/reads-v2.md`; `docs/agent-hosts.md`; `docs/bench.md`; the seven code
reviews `code-M6-a`, `code-M6-b`, `code-M6-c`, `code-M6-c2`, `code-M8-a`,
`code-M8-a2`, `code-M8-a3`; then the code itself — the `Commands` enum and
`Cli::validate`, `output/registry.rs` and `output/json_schema.rs`,
`canvas-core::{plan, coord, events}`, `canvas-api::governor`,
`canvas-core::sync::{pages, discussions, inbox}`, `crates/canvas-cli/src/mcp/`,
`commands/{watch, notify, schema, submit, discussions}.rs`,
`store/migrate.rs`, `config.rs`, `session.rs`, the four `Cargo.toml` files, the
registry fixtures, and the shipped skill.

Placement: §§20–23 sit after §19 and before Appendix A, so the section numbers
still ascend and the appendices stay last.

## Sections added

### §20 Operation plans and approval (M6-a)

The `plans` and `approval_handles` tables with their full column lists, the
journal's `plan_id`/`approval_json` columns and the partial unique index, and
the note that SQLite keeps `NULL` distinct so a legacy journal never collides.

The five plan states — `prepared`, `approved`, `executed`, `expired`,
`invalidated` — with the transitions each is reached from, and the rule that
every transition is one `BEGIN IMMEDIATE` with a `WHERE … state IN (…)` guard.

The 15-minute admission expiry, and that it gates **first admission only**: a
status read or a replay of an executed plan is answered with its journal.

The approval record (`channel`, `at`, `consumer?`, `plan_sha256`), with
`yes-flag` recording an explicit `--yes` and never claiming an interactive
decision.

The handle binding: a random server-issued handle bound to one plan, one
consumer, and one deadline, spent single-use in the same transaction that
approves the plan; and the six separate refusals (`unknown_handle`,
`handle_for_another_plan`, `handle_already_used`, `wrong_consumer`,
`handle_expired`, `plan_digest_mismatch`), so no answer says whether another
handle would have worked.

What `plan_sha256` covers, and what it deliberately excludes — the outbound
bytes and the local file paths, because the bytes are pinned by `sent_sha256`
and the files by their `sha256`.

The ten frozen observations, sorted where they are lists.

`prepare`: pre-flight step 1, admission taken for its own pre-flight only, and
**no lock held while a person considers a plan**.

Execute as ten ordered rules, including the 5-second wait for this plan's own
concurrent execute, the re-read under admission, the observation comparison
before the eligibility check, and the one transaction guarded twice.

The human `submit` on top: same v1 contract, one extra pre-flight `GET`,
`tty` for an answered prompt and `yes-flag` for `--yes`, and a declined prompt
cancelling the plan.

The exit-mapping table, and `replayed: bool` on `submit@1` (§19 item 17).

### §21 Agent surface (M6-b, M8-a2)

`canvas schema`: class A, raw output, `--json` exit 2, unknown operand exit 6,
and the three output forms — envelope, stream summary, stream line — in one
table. The stream line carries `line` only, with its `schema` field pinned by
`const`.

`canvas mcp`: one identity **and** one generation bound at startup, exit 3
without an identity, a two-second `identity.json` poll and exit 13 when it
changes, and one JSON-RPC message per line on stdout.

The two protocol revisions, `2026-07-28` (primary, no handshake) and
`2025-11-25` (adapter), and the explicit refusal of any other.

The 30-tool catalog grouped by effect, with the annotation rule
(`readOnlyHint` follows the effect; `destructiveHint` is false throughout).

What the catalog does not contain, and why each absence is unreachable by name
and by argument — `deny_unknown_fields`, `dest: None`, `force: false`,
`ics: None`, `Launch::No`, `yes: false`, and the refusal of `text: "-"`.

Results: the whole §7 envelope in `structuredContent` and in the text block;
`isError` for `error`, `refused`, `mismatch`, and `recovery`, with `partial`
staying a success; and the `"type": "object"` beside the `oneOf`.

`ttlMs` and `cacheScope` under `dev.canvas-cli/*` `_meta` keys, with the rule
that anything unresolved is zero.

The four resources by identity and generation.

The four-step approval round trip, and the `approval_required` refusal a host
without elicitation gets.

The catalog size: 30 tools, 220 855 bytes, about 55 224 estimated tokens
(§19 item 19).

The shipped skill and its five workflows, and the `docs/agent-hosts.md`
pointer, with the caveat that both connected third-party hosts negotiated
`2025-11-25` and neither exercised the approval round trip.

### §22 Coordinator, events, `watch`, `notify` (M6-c, M6-c2, M8-a3)

The four shared things and their lock paths; the rule that no lock file is
ever deleted except by `identity remove`.

Permits: one slot file held for a request's whole duration, the 2–40 ms poll,
and the 5-second I/O grace that degrades to this process's own cap.

The shared governor row and its **two** merge rules — conservative before an
admission decision, sample-preserving right after a sample is applied — plus
issue-counter realignment and the row-owned silence reset. The second
`state.sqlite` connection is named, with §19 item 22.

Single-flight: the injective `~hh` lock-name encoding, the 200-character
digest fallback, the 30-second waiter, and exactly what an expired waiter
serves.

Foreground interest and the priority rule, including that a terminal
`outcome_unknown` journal is **not** in flight (§19 items 23, 25, 29).

The observation protocol as three steps with the two kill windows named; the
observation id `<dataset>:<scope>:<fetch_log rowid>:<fetched_at>`.

The baseline rules, the four shapes in one table with their allowlisted
payloads, the field split that produces `due.changed`, `grade.posted`, and
`grade.changed`, and why the unread count has no removal kind.

Journal events written inside the journal's own transaction.

The eleven kinds, the `AUTOINCREMENT` cursor and why it matters, 30-day
retention, and the cursor rules including `resync_required` at exit 0.

`canvas watch`: class D, `--offline` exit 2, `--json` exit 2 naming `--jsonl`,
resident consumer, the tick order ending with `inbox_unread` last, the 30 s
tick and 30 s→15 min backoff, the whole-log replay without `--since`
(§19 item 24), and the `watch@1` closing envelope with its three `skipped`
values.

`canvas notify`: class B, raw output, the seven groups, the cursor that moves
only after the lines are written, and the missing desktop backend
(§19 item 34).

MCP subscriptions: the `dev.canvas-cli/cursor` `_meta` key, the durable
position, the invalidation map as a table, the once-per-batch rule, why
`context/<handle>` is readable but not subscribable, and the rule that a
host-named cursor never replaces the stored one.

### §23 Richer reads (M8-a, M8-a2)

All eight commands with their exact requests and their dataset, scope, and
TTL; the statement that every one is class C, every request is a `GET`, and
nothing is marked read; and that `sync` does not refresh them.

Fifteen reading rules, each one a short normative sentence.

Bodies, embedded content, and file references: what becomes an `embedded`
row, what becomes a `files` or `external_links` row, the capability-stripping
rule, and the 64 KiB per-document bound with `truncated: true` and exit 12.

The exit table, with the initial-post gate and the auth/throttle carve-out.

The rubric extension, the cache migration and the three config keys, and the
eight schemas.

### §24 and §25

One-line placeholders. M7-a and M7-b (companion, broker, presence) and M8-b
(discussion and inbox writes) are in flight; no part of either is on `main`
(no `extension/` directory, no `docs/companion.md`, nothing in `git log main`).

## Tables extended, not duplicated

| Section | What changed |
|---|---|
| Header | `draft v0.8` → `draft v0.9`, 2026-09-10; says which sections are the v1 contract, which are as-built, and which are placeholders |
| §5 Commands | new sub-block "Commands added after v1" with the twelve command forms; the v2-reserved line no longer reserves `inbox`, `discussions`, `discussion`, or `notify`, because they exist |
| §5 Behaviour notes | three lines pointing the new commands at §21, §22, §23 |
| §5 Command classes | `schema` → A; `notify` → B; the six M8-a reads plus `inbox show` and `inbox unread-count` → C; `watch` → D; a sentence saying `canvas mcp` has no class of its own |
| §7 Output contract | `schema` and `notify` join the raw-output list; a new bullet for `watch --json` exit 2 and the separate `--jsonl` contract |
| §9 Config and paths | the three coordinator lock paths; a reserved row for the M7-a broker endpoint; `ttl_pages`, `ttl_discussions`, `ttl_inbox` in the `[cache]` example; a note that no `bridge.*` key exists yet and that the eight post-v1 state tables are out of `cache clear`'s reach |
| §10 Cache, state, and sync | seven new dataset rows with scope, fetch, TTL, and complete rule; a new "Migration list" subsection naming `0001_initial`, `0002_reads`, `0002_plans`, `0003_events` and the two `user_version` values; the one-SQLite-thread bullet now names the coordinator exception |
| §13 Architecture | one sentence naming `cargo xtask bench --watch` and `--mcp`, and saying neither has a §13 target of its own |
| §14 Errors and exit codes | a "Refusal reasons" table for the four `details.reason` values on exit 8, and the note that `in_progress` is a message, not a reason. **No new exit code** |
| §18 Milestones | one sentence: the round table stops at R5, and REPORT §4 holds the rounds after it |
| §19 Open questions | items 34, 35, 36 added; every existing item verbatim |
| Appendix B | title drops "(v1)"; a second table with nine post-v1 endpoint rows; a line on why `/view` is never used |
| Appendix C | new subsection "What the post-v1 rounds changed": the seven merged packages, their sections, their review files, what is still in flight, and the fate of `docs/reads-v2.md` |
| Appendix D | title drops "(v1)"; five new shared objects (`Listing`, `Embedded`, `FileRef`, `ExternalLink`, `Participant`); eleven new schema rows (`plan@1`, `watch@1`, and the eight M8-a schemas, plus `error@1` gaining `details.reason`); the `event@1` line document with an example, marked as not a `result`; and a table of the five additive field sets that keep their `@1` |

## Appendix A corrections

The brief said Appendix A was "already updated; verify". It was verified line
by line against the four manifests, and four things were wrong or stale.

| Row | Was | Now |
|---|---|---|
| `tokio` | `(rt, macros, fs, time, sync, io-util, signal, net)` … "`net` for the broker socket (M7-a)" | the real split — workspace `rt, macros, fs, time, sync`, per crate `io-util, signal, rt-multi-thread, process` — and the true statement that **`net` is enabled today only in `canvas-api`'s dev-dependencies**, with the broker socket that needs it pointed at §24 |
| `rmcp` | `=3.2.0`, `server`, `transport-io`, `local` | the full set: no default features; `server`, `client`, `macros`, `elicitation`, `transport-io`, `transport-async-rw`, `schemars`, `local` |
| `html2text or htmd` | "latest (pick in M1-c)" | `htmd` 0.5.5 (+ `markup5ever_rcdom` 0.38.0); M1-c's pick recorded |
| `fs4`, `sha2`, `tracing` | "latest 0.13.x", "0.10.x", "latest" | 0.13.1, 0.10.9, 0.1.44 / 0.3.23 |

Added: a `uuid` 1.18.1 row (journal ids, plan ids, approval handles, identity
generations), and a closing sentence naming the seven direct dependencies the
table still omits — `getrandom`, `unicode-normalization`, `httpdate`,
`rpassword`, and, for tests and `xtask` only, `tokio-rustls`, `tempfile`,
`url` — plus the statement that no post-v1 package added a dependency after
`rmcp` and `schemars`.

## §19 items 34–36

Every existing item is verbatim; none was resolved, reworded, or renumbered.

| # | Difference |
|---|---|
| 34 | **`canvas notify` has no desktop backend.** REPORT §3.6 has desktop alerts consuming events. `--stdout` is the only backend, because `notify-rust`'s macOS path is Objective-C FFI and the workspace forbids `unsafe`. Without `--stdout` the command writes the same lines to stdout and warns on stderr, so it never claims a notification it did not post. |
| 35 | **The eight M8-a schemas have no typed arm in the schema generator**, so their `canvas schema` pages are inferred from the registry fixture and describe nullable fields as non-nullable. The page is honest (`result_source: "registry fixture"`), but a strict host validator would reject a legitimate answer. Raised by the M8-a2 review and never recorded. |
| 36 | **`canvas schema --list` names commands that do not exist.** It derives a command name from the schema id, so `conversation@1` is listed as `conversation` and `inbox_unread@1` as `inbox unread`, while `canvas schema "inbox show"` exits 6. Confirmed by running the command. Raised by the M8-a2 review and never recorded. |

## Two facts recorded in `SPEC-CHANGES-v0.9.md`, not changed in the SPEC

- **`canvas watch` behaves as class D, not class C.** It calls
  `globals.open_session()` (an online session, so a token is needed) and
  refuses `--offline` with exit 2, which is §8's class-D rule. The module's own
  doc comment at `crates/canvas-cli/src/commands/watch.rs:1` says class C. §5
  now records the behaviour; the comment is code and was not touched.
- **The binary's `--help` epilogue is out of date.** `AFTER_HELP` in
  `crates/canvas-cli/src/cli.rs` lists the v1 commands plus `watch` and
  `notify`, and omits `pages`, `page`, `syllabus`, `discussions`, `discussion`,
  `inbox`, `schema`, and `mcp`. This pass touched no code.

Two more observations are in the same section: Appendix A's seven previously
unlisted direct dependencies, and the `tokio` `net` finding above.

## `docs/reads-v2.md`

Folded into §23 in full — the contract table, the config keys, the cache
migration, the schemas, the body and reference rules, the exit table, all
nineteen M8-a decisions, and the M8-a3 event section (which is §22). The file
is now a pointer at §23, §22, and §21, and names the open §19 items 26, 27,
28, and 35. Only `tasks/` still links to it, and `tasks/` was not touched.

## Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo nextest run --all-features` | **731 run, 731 passed, 0 skipped** |

731 is the same count as the M8-a3 merge. That is the point of the second
gate: it is the evidence that this pass touched no code.

`git merge main` reports "Already up to date". `git status --short` is empty.
Nothing was pushed and nothing was merged into `main`.
```
