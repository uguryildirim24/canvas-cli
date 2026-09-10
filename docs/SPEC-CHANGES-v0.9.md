# SPEC v0.9 — what changed, section by section

Consolidation pass 1, 2026-09-10, lane `w3`. Brief:
`tasks/spec-v09-consolidation.md`.

The job was to fold what is **on `main` now** into `docs/SPEC.md`, with no new
decision. The code and its tests are the truth for "as built";
`docs/agent-ux/REPORT.md` is the truth for intent. Nothing here resolves a §19
item, and no §19 item was reworded or removed.

One line per section, so this can be checked fast.

## Added

| Section | What it now says | Source read |
|---|---|---|
| §20 Operation plans and approval | the `plans` and `approval_handles` tables, the five plan states and their guards, the 15-minute admission expiry, the approval record, the handle binding and its six refusals, what `plan_sha256` covers, the ten execute rules, the human `submit` on top, the exit mappings, and `replayed` | `canvas-core::plan`, `store/migrate.rs`, `commands/submit.rs`, review M6-a |
| §21 Agent surface | `canvas schema` and its three output forms; `canvas mcp` — two protocol revisions, the 30-tool catalog by effect, what is absent and why it is unreachable, results and `isError`, `ttlMs`/`cacheScope`, the four resources, the approval round trip, the catalog size; the skill and its five workflows; the host-matrix pointer | `mcp/*`, `output/json_schema.rs`, `commands/schema.rs`, `skill/`, reviews M6-b and M8-a2 |
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

## Verified, and worth the owner's eye

These are facts found while checking the code. None of them is a SPEC change.

- **`canvas watch` behaves as class D, not class C.** It opens an online session and refuses `--offline` with exit 2, which is §8's class-D rule. The module's own doc comment says class C. §5 now records the behaviour; the comment in `crates/canvas-cli/src/commands/watch.rs:1` still says class C.
- **The binary's `--help` epilogue is out of date.** `AFTER_HELP` in `crates/canvas-cli/src/cli.rs` lists the v1 commands plus `watch` and `notify`, and omits `pages`, `page`, `syllabus`, `discussions`, `discussion`, `inbox`, `schema`, and `mcp`. This pass touched no code.
- **Appendix A still omitted seven direct dependencies** before this pass: `getrandom`, `unicode-normalization`, `httpdate`, `rpassword`, `tokio-rustls`, `tempfile`, `url`. They are now named in a closing sentence rather than given rows, because they predate the post-v1 rounds.
- **`tokio`'s `net` feature is not a production feature on `main`.** It is enabled only in `canvas-api`'s dev-dependencies. The Appendix A note now says so and points at §24.

## Gates

```
cargo fmt --all --check      # clean
cargo nextest run --all-features
```

The test count is unchanged from the M8-a3 merge, which is the point: this pass touched no code.
