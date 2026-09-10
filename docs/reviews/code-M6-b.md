# Code review — M6-b (agent adapters: `canvas mcp`, `canvas schema`, the shipped skill), branch `lane/w2`

Reviewer: Claude Opus 5 (high). Base: `9ddbbf5` (merge of `main` into
`lane/w2`). Package brief: `tasks/m6b-agent-adapters.md`. Reviewer brief:
`tasks/review-code-m6b.md`. Contract: `docs/agent-ux/REPORT.md` §3.2 (all of
it) and §3.5, `docs/SPEC.md` §7 and §14 for envelopes and exits, §15, §16,
Appendix A and Appendix D, and the coordinator reading recorded as §19 item
17.

## Verdict

**MERGE.** The approval guarantee holds under attack: no tool and no argument
can dispatch a remote write without a consumed, single-use, server-issued
handle, because `plan::execute` re-reads the plan under the admission lock and
refuses anything that is not `approved` with an `approval` row and a matching
digest, identity, and generation — and I confirmed by request count that a
host declaring no elicitation gets the `approval_required` refusal while every
request the mock server saw is a `GET`. The catalog is exactly the 22 names
REPORT §3.2 lists, I enumerated it myself, and nothing forbidden is reachable
through a name or an argument. Five `review(M6-b):` commits fix four real
defects I found, the largest of which made two tools advertise an output
schema that rejects their own successful result; all five gates are green at
646 tests. Six items are listed under "Needs a decision"; none blocks the
merge, and the largest is that `docs/SPEC.md` Appendix A does not yet carry
the two dependencies this package adds, which the worker was forbidden to add.

## Gate results

Run with `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/rev-m6b`
at `6df6fa4` (after the fixes).

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass |
| `cargo nextest run --all-features` | pass — 646 tests run, 646 passed, 0 skipped |
| `cargo deny check` | pass — advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | pass |
| `cargo xtask bench --mcp --runs 3` | pass — every target ok |

At the base (`9ddbbf5`) the same five gates were already green with 643 tests.
The three added tests are mine.

`cargo xtask bench --mcp --runs 3`, rerun at `6df6fa4`: warm `todo.list` round
trip p50 2.2 ms, p95 2.4 ms against a 100 ms target; catalog 22 tools,
167 539 bytes, about 41 891 tokens per `tools/list`. At the base the catalog
was 166 501 bytes, which reproduced the worker's reported number exactly; the
1038-byte increase is the two receipts tools now carrying their own output
schemas. Every §13 timing target still passes.

## What I verified against the contract

**No remote write without a consumed approval.** `submission.execute` is the
only tool with `Effect::RemoteWrite`, and a test pins that it is the only one.
`canvas-core::plan::execute` refuses before it opens the network unless the
plan is `approved` *and* carries an `approval` row, and it re-reads the plan
and re-checks both under the admission lock after the revalidation `GET`, so a
decline that lands mid-flight is caught. `approve` requires a handle that
exists, belongs to that plan, has that consumer, has not expired, and has no
`used_at`. An argument named `handle`, `approved`, or `yes` is a `-32602`
argument error, because every argument struct is `deny_unknown_fields`.

**A host with no elicitation dispatches nothing.** `agent_execute` on a
prepared plan never builds a client; it issues a handle and returns. The
refusal is `outcome: refused`, exit 8, `details.reason: approval_required`,
with the handle and the plan id. The test counts wiremock requests: zero
`POST`, and every request the server saw is a `GET`.

**The catalog is REPORT §3.2.** I enumerated `catalog::specs()` by hand
against the report's four rows plus navigation: 14 coursework, 3 organization,
3 submission, 1 evidence retirement, `open.url` — 22, in the report's order.
Absent, and unreachable by name or argument: credentials, token reveal,
identity administration, arbitrary HTTP or shell, `--yes`, cache clearing,
`download --force`, and every browser action. `download.*` hard-codes `dest:
None` and `force: false`; `calendar.list` hard-codes `ics: None`; `open.url`
passes `Launch::No` and the launch is short-circuited before `open::that` is
reached; `submission.prepare` hard-codes `yes: false` and refuses `text: "-"`
because stdin is the transport.

**Protocol.** Both `2026-07-28` (no handshake, per-request `_meta`,
`server/discover`) and `2025-11-25` (through `initialize`) work over a real
pipe. `2024-11-05`, `2025-03-26`, `2099-01-01`, and an unknown per-request
version all fail with `-32022`; the handler refuses explicitly rather than
letting the SDK downgrade silently.

**Resources.** `canvas://<key>/<generation>/<path>`. `path_of` rejects a
foreign key and a second generation before it resolves anything, so a URI
minted for one generation addresses nothing after a re-login, and the instance
itself stops with exit 13 when `identity.json` changes.

**Tool result equals `--json`.** `every_tool_returns_the_envelope_the_cli_prints`
covers 21 of 22 tools against the `canvas` invocation behind each one and
compares `structuredContent`, the text block, and `isError`; the table is
diffed against the catalog, so a new tool with no command behind it fails.
`submission.execute` is excluded and named. `Handled` serializes both sides
through the same `serde_json`, so drift is not possible by construction.

**`ttlMs`.** The budget is the minimum remaining TTL over every freshness row,
and zero for an empty list, a stale or incomplete row, an unparseable
`fetched_at`, an expiry in the past, or a dataset with no TTL group. I checked
the dataset-name map against every dataset literal in `canvas-core` and
`canvas-cli`: all 17 are covered, so no real result silently falls to zero.

**`canvas schema`.** Comes from the registry — nothing hand-written; typed
results through `schemars`, the rest inferred from the registry fixture and
labelled `result_source`. `--json` is exit 2, an unknown command is exit 6,
and no operand is exit 2.

**The skill.** Names exactly the catalog, diffed both directions, with the two
submission tools confined to the approval workflow. Its exit table covers 0–13
including 11, which a declined approval returns. No forbidden flag appears
except as an explicit statement that it does not exist.

**`docs/agent-hosts.md` claims only what was run.** I compared every row and
every "how it was produced" paragraph against the worker's transcript: they
agree, including the two things the file is careful *not* to claim — Codex's
handshake (`codex mcp list`/`get` never launch the server) and the approval
form in a third-party host. Both connected hosts negotiated `2025-11-25`; the
file says so and says the primary revision is exercised only by the project's
own clients.

**The host runs left nothing behind.** `security find-generic-password -s
canvas-cli` finds no item; `~/.config/canvas-cli`,
`~/.local/share/canvas-cli`, and `~/Library/Application Support/canvas-cli` do
not exist; `~/.claude.json`, `~/.cursor/mcp.json`, and `~/.codex/config.toml`
carry no `canvas` MCP server entry. The only `canvas-cli` strings left are
this repository's own project keys in the two host config files, which are the
owner's.

**§15.** The adapter writes nothing to disk and holds no token: the token
never reaches an envelope, and the equivalence test asserts the token string
is absent from every tool's text block.

## Defects found and fixed

| # | Severity | Where | What was wrong | What I changed | Commit |
|---|---|---|---|---|---|
| 1 | High | `crates/canvas-cli/src/mcp/catalog.rs:102`, `crates/canvas-cli/src/output/json_schema.rs:169` | `receipts@1` has three different result documents and the registry holds one entry per shape, but `document_for_schema` took the first. `receipts.show` and `receipts.acknowledge` therefore declared an `outputSchema` requiring `journals` while returning `{journal, receipt}` and `{journal_id, acknowledged_at}`. Every property is `required`, so a host validating `structuredContent` against `outputSchema` rejects every successful call of either tool. The same lookup made `canvas schema --list` print four indistinguishable `receipts` rows (and three `cache`, three `config`, two `identity`), and left three of the four receipts shapes with no way to be asked for. | `ToolSpec` gained a `variant`; `document_for_schema` takes it; `entry_for_schema` selects the shape; the three receipts tools name theirs. `canvas schema --list` names the variant, and `entry_for_command` resolves `receipts show`. A bare command still answers with the shape it prints by itself. Two tests: one pins each tool's declared shape against what it returns, one pins that no listing row repeats. | `e7a5938` |
| 2 | Medium | `crates/canvas-cli/src/mcp/resources.rs:169` | `/context/<handle>` put the refusal reason in `result.code` and left `details.reason` absent, while every plan refusal uses code `refused` with `details.reason`. REPORT §3.2 lists `not_attached` in the same `reason` column as `expired` and `approval_required`, so a host needed a second rule to read the same kind of answer. | Code `refused`, `details.reason: not_attached`, consumer kept in `details`. Unit and wire tests updated to assert the reason where the report puts it. | `81d53e2` |
| 3 | Medium | `crates/canvas-cli/src/mcp/server.rs:347` | `call_tool` treated any `requestState` as the second half of an approval round trip, whatever tool the retry named. A retry naming `todo.list` or `receipts.acknowledge` would record the decision against the plan the state names and, on accept, dispatch the submission — a call that never asked for an approval would be the one that consumed it. Not reachable through a tool argument (`requestState` is a protocol field), but it makes a host bug or a replayed state into a dispatch. | `submission.execute` is the only tool that returns `input_required`, so it is the only tool whose retry may carry a state; any other is an argument error and the plan is left untouched. A test drives three misrouted retries, asserts no `POST`, and then answers the tool that did ask. | `3c5ef62` |
| 4 | Medium | `crates/canvas-cli/src/mcp/result.rs:96` | A tool result was `isError` for `error`, `refused`, and `mismatch` but not for `recovery`. §14 ranks exit 9 above `mismatch` (10) and `refused` (8) in its precedence for a completed command, so the outcome the CLI ranks highest was the one an agent host rendered as a plain success — and exit 9 is a submission whose outcome is unknown, the case §12.2 is most careful about and the one the skill tells a model never to retry. | `recovery` joins the domain failures; the match is now exhaustive, so a new outcome cannot be forgotten. `partial` stays a success with gaps, which is the reading the package already documented and tested; the wire test now says so instead of relying on the table happening to contain no exit 12. | `c8e2758` |
| — | — | `docs/bench.md` | Not a defect: the generated report is rewritten by every run. | Refreshed after the reruns. | `6df6fa4` |

Nothing else needed changing. The worker's structure is sound and I kept it:
the `Handled` extraction is the right shape for the "one implementation per
command" requirement, the plan/approval split in `submit.rs` gives the CLI and
the adapter one enforcement path, and the test suite is unusually good — it
drives a real stdio pipe with hand-written JSON-RPC rather than the SDK's own
client, which is what makes the protocol claims worth anything.

## Needs a decision

1. **Appendix A does not list `rmcp` or `schemars`.** The package adds two
   direct dependencies, both pinned with `=` as the appendix requires
   (`rmcp = "=3.2.0"`, `schemars = "=1.2.2"`), plus 21 transitives. The worker
   was forbidden to edit `docs/SPEC.md`, so the appendix and the code now
   disagree about what the workspace depends on. The `local` feature of `rmcp`
   is load-bearing: the command cores are `!Send`, so the service runs in a
   `LocalSet` and the handlers would not compile without it.
2. **`chrono` arrives transitively through `rmcp`.** §7 timestamps go through
   `jiff`, and the workspace does not otherwise use `chrono`. Two time
   libraries in the lock file is either acceptable or an appendix exception;
   it is not something the code can decide.
3. **The catalog costs about 41 891 tokens per `tools/list`.** Dominated by
   the self-contained output schemas — the whole §7 envelope in both shapes
   with every subschema inlined, because a host validator reads a tool
   definition on its own. The generated bench section names this as the number
   to beat. Trimming it means either `$ref`s a host may not resolve or a
   smaller catalog, and both are owner calls.
4. **`schema@1` has no registry entry of its own.** REPORT §3.2 adds
   `schema@1` to the registry table, and `SCHEMA_SCHEMA` is emitted as the
   `contract` field of every document, but `all_schemas()` has no `schema@1`
   row — it cannot have one, because the document is raw output with no §7
   envelope and no `result`. So `canvas schema schema` exits 6. That reads
   correct to me, but the report's table implies a row.
5. **`readOnlyHint` on `download.plan` and `open.url`.** Both are `true`
   here: `download.plan` is `dry_run` and writes nothing, and `open.url`
   resolves without launching. REPORT §3.2 says annotations describe effects
   rather than command classes, which supports this reading, but the same
   sentence lists "downloads, and navigation" among the things that are not
   pure reads. I left it: the effect is the truth, and the hints are
   documentation, not enforcement. Worth one line from the owner if the
   category was meant literally.
6. **`jobs` is unbounded on the agent surface, as it is on the CLI.**
   `download.run` preserves the v1 `--jobs` argument per REPORT §3.2, so a
   model can ask for an arbitrary parallelism. The practical bound is the file
   count and the §11 governor, so this is not a hole, but if agent-facing
   arguments are meant to be clamped where the CLI's are not, `jobs` is the
   only one.
