# Code review + fix — M6-b on branch lane/w2 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M6-b
on branch `lane/w2` (worktree `/Users/rolfie/projects/canvas-cli/.worktrees/w2`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /Users/rolfie/projects/canvas-cli/.worktrees/w2`. Read the package brief `tasks/m6b-agent-adapters.md` and the SPEC sections it
   cites. First run `git merge main` (expect nothing to do; resolve if needed and commit). Then read `git log main..HEAD --stat` and the full diff. Contract: `docs/agent-ux/REPORT.md` §3.2 (all of it) and §3.5 (the approval handle), `docs/SPEC.md` §7 and §14 for envelopes and exits, and the coordinator reading recorded as §19 item 17 (an executed plan replays the journal's `submit@1` with `replayed: true`). Attack in particular: can any MCP tool or argument dispatch a remote write without a consumed approval handle, and does a host that declares no elicitation get a `approval_required` refusal with nothing dispatched (count wiremock requests)? Is the catalog exactly REPORT §3.2 with nothing forbidden reachable (credentials, token reveal, identity administration, arbitrary HTTP or shell, `--yes`, cache clearing, `download --force`, browser actions) — enumerate it yourself, do not trust the allowlist test alone? Do both declared protocol versions handshake and is any other refused? Are resources namespaced by identity **and** generation so a second generation sees nothing? Does every tool's success and domain-error envelope equal the CLI's `--json` for the same fixture (snapshot diff), and do `structuredContent` and the text content carry the same JSON? Is `ttlMs` never above the remaining freshness of the oldest dataset and 0 for anything unresolved? Does `canvas schema` come from the registry and refuse `--json` with exit 2? Does the skill name exactly the catalog and no forbidden flag, and does `docs/agent-hosts.md` claim only what was run (compare with the transcript of what the worker says it ran)? Verify that the worker's host runs left nothing behind: no keychain entry, no host config entry, no files under `~/.config/canvas-cli` or `~/.local/share/canvas-cli` that are not the owner's own. Rerun `cargo xtask bench --mcp --runs 3`. The worker's final report, for reference:

```
# M6-b — agent adapters — final report

Lane w2, branch `lane/w2`, worktree `/Users/rolfie/projects/canvas-cli/.worktrees/w2`.
Reported 2026-09-10. Nothing pushed, nothing merged into `main`.

All six gates pass after `git merge main`:

```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features          # 643 tests run, 643 passed
cargo deny check                          # advisories ok, bans ok, licenses ok, sources ok
cargo +1.88 check --workspace --all-targets
cargo xtask bench --mcp --runs 3          # every target ok
```

```
$ git status --short
(clean)
```

---

## 1. Delivered items

### Reusable command handlers

Every command core returns one §7 envelope as a `Handled` value, which carries
the envelope and the renderer for human output. The CLI turns it into terminal
output and a process exit; `canvas mcp` reads the same envelope and serializes
it into a tool result. There is one implementation per command and the adapter
never shells out to `canvas`. Each command's renderer stayed where it was.

### `canvas schema`

`canvas schema <command>` prints the `schema@1` document of that command's
`--json` output: the envelope in both branches, plus `result` and `error` on
their own. `canvas schema --list` prints the registry. `--json` is a usage
error with exit 2, and an unknown command is a resolution error.

### `canvas mcp`

An stdio Model Context Protocol server on rmcp `=3.2.0`.

- **Identity and generation binding at startup.** One instance serves one
  identity. It refuses to start without one and exits 3. A watchdog polls the
  identity document every two seconds and stops the instance with exit 13 when
  the key or the generation changes.
- **Two revisions.** `2026-07-28` is primary: no handshake, a per-request
  `_meta` carrying the version, client identity and capabilities, and
  `server/discover`. `2025-11-25` is reached through `initialize` and is the
  adapter. Any other revision fails the handshake with rmcp error `-32022`.
  The refusal is explicit, because the SDK would otherwise answer an unknown
  `initialize` version with its newest legacy version.
- **22 tools**, in the REPORT §3.2 order: `courses.list`, `course.get`,
  `todo.list`, `assignments.list`, `assignment.get`, `grades.get`,
  `files.list`, `modules.list`, `announcements.list`, `announcement.get`,
  `calendar.list`, `submission.get`, `receipts.list`, `receipts.show`,
  `sync.run`, `download.plan`, `download.run`, `submission.prepare`,
  `submission.execute`, `submission.reconcile`, `receipts.acknowledge`,
  `open.url`. Absent by design: credentials, token reveal, identity
  administration, arbitrary HTTP or shell, `--yes`, cache clearing,
  `download --force`, and every browser action.
- **Annotations describe effects.** `readOnlyHint` is true only for a pure
  read, `destructiveHint` is never true, and `idempotentHint` is set where it
  holds. The hints are documentation; the plan and approval core is the
  enforcement.
- **Results.** `structuredContent` is the full §7 envelope and the text block
  is the same JSON. A domain failure keeps the envelope with its `outcome` and
  `exit` and is marked `isError`. A protocol or argument failure is a JSON-RPC
  error, because then the tool never ran. Every output schema admits both
  shapes.
- **Private results** carry `cacheScope: private` and a `ttlMs` bounded by the
  remaining freshness of the oldest dataset in the envelope, and 0 for
  anything unresolved.
- **The approval flow.** `submission.execute` on an approved plan runs. On a
  prepared plan it issues a handle and returns `input_required` with
  `requestState = { plan_id, handle }` and one `elicitation/create` request.
  The host's retry echoes the state and carries the handle in
  `inputResponses`. Accept records the approval with channel `elicitation` and
  the session's consumer id, then executes. Decline and cancel invalidate the
  plan. A host that declares no elicitation gets a domain `approval_required`
  refusal with the handle, and nothing is dispatched.
- **Resources** are namespaced by identity **and** generation:
  `canvas://<identity-key>/<generation>/todo`, `/receipts`,
  `/course/{course_id}/assignments`, and `/context/{consumer_handle}`, which
  exists and answers `not_attached` as a §7 refusal. Reads go through the same
  handlers. A second generation and a foreign key see nothing.
- **Subscriptions.** `subscriptions/listen` is declared and accepts the
  resource-list category only.

### The shipped skill

`skill/canvas-cli/` holds `SKILL.md` plus one file per workflow:
`organize-the-week.md`, `read-an-assignment.md`, `prepare-and-submit.md`,
`reconcile-an-unknown-outcome.md`, `download-course-files.md`. `SKILL.md`
carries the envelope reading order, the identity model, the workflow table,
the §14 exit-code and recovery table, freshness and cache guidance, the
resource list, and the MCP setup for Claude Code, Codex, and Cursor, with the
CLI as the fallback. `dist-workspace.toml` puts `skill` in the release
archives, and the README points at it.

### Documentation

- `docs/agent-hosts.md` — the host matrix, section 2 below.
- `docs/bench.md` — an `## Agent surface (canvas mcp)` section, section 3
  below.

### Tests

643 tests pass. The ones this package adds:

- Every tool returns the envelope the CLI prints. One table names all 22 tools
  with the `canvas` invocation each wraps, and the test compares
  `structuredContent`, the text block, and `isError` for the same arguments —
  an answer, a refusal, or the usage error a networked command gives while
  `--offline`. It fails when a new tool appears with no command behind it.
  `submission.execute` is excluded and named: it takes a stored plan, not a
  course.
- Both revisions handshake, and `2024-11-05`, `2025-03-26`, `2099-01-01` and
  an unknown per-request version are refused with `-32022`.
- The catalog holds no forbidden tool and no forbidden argument, enumerated
  against an allowlist, with `assume_not_submitted` the only evidence-retiring
  argument and `submission.execute` the only remote write.
- Accept, decline, cancel and replay through `input_required`, over a real
  pipe: a decline and a cancel leave the mock server with no POST; an accept
  posts exactly once and records the `elicitation` channel with the host's
  consumer id; a second execute replays that journal and posts nothing more.
- The no-elicitation refusal dispatches nothing: the reason is
  `approval_required`, the handle travels, and every request the mock server
  saw is a GET.
- An approval cannot be asserted by a tool argument, and a forged request
  state names a handle that was never issued.
- `--text -` is refused, because stdin carries the protocol.
- Resources are private to the identity generation.
- `canvas schema` matches the registry and rejects `--json` with exit 2.
- The skill's command names match the catalog, diffed in both directions.
- The server refuses to start without an identity (exit 3), and a replaced
  identity stops the instance (exit 13).

---

## 2. The host runs

Recorded 2026-09-10 on macOS 26.6.2, arm64. Every host launched the same
wrapper, which `tee`s both directions of the stdio session to a file, so each
row is read off a session log rather than inferred from a screen.

| Host | Version | Connected | Revision | `tools/list` | `resources/list` | Elicitation declared | `input_required` exercised |
|---|---|---|---|---|---|---|---|
| Claude Code (CLI) | 2.1.267 | yes | `2025-11-25` | 22 tools | not requested | yes, `elicitation: {}` | no |
| Cursor (`cursor-agent`) | 2026.09.08-6caf4ff (client id `Cursor 1.0.0`) | yes | `2025-11-25` | 22 tools | 2 resources | yes, `elicitation: { form: {} }` | no |
| Codex CLI | 0.153.4 | not exercised | — | — | — | — | no |
| Project harness (`crates/canvas-cli/tests/mcp.rs`) | this build | yes | `2026-07-28` and `2025-11-25` | 22 tools | 2 resources + 2 templates | yes and no, both cases | yes |
| Project harness (`cargo xtask bench --mcp`) | this build | yes | `2026-07-28` | 22 tools | not requested | no | no |

### How each row was produced

- **Claude Code.** `claude mcp add … -- <wrapper>` in a scratch directory,
  then `claude mcp list`, which health-checks the server. The log shows
  `initialize` with `protocolVersion: 2025-11-25` and
  `capabilities.elicitation: {}`, then `notifications/initialized` and
  `tools/list`.
- **Cursor.** A `.cursor/mcp.json` in a scratch workspace, then
  `cursor-agent mcp enable` and `cursor-agent mcp list-tools`. The log shows
  `initialize` with `protocolVersion: 2025-11-25` and
  `capabilities.elicitation: { form: {} }`, then `tools/list` and
  `resources/list`.
- **Codex CLI.** `codex mcp add` accepted the stdio configuration and
  `codex mcp list` and `codex mcp get` showed it enabled, but neither command
  launches the server: no process started and no session log exists. Nothing
  about Codex's handshake is claimed.
- **The project harnesses.** `tests/mcp.rs` speaks hand-written JSON-RPC over a
  real pipe and is the only client that exercises the whole approval round
  trip. `cargo xtask bench --mcp` is a second hand-written client.

### What the two revisions mean in practice

**No third-party host asked for `2026-07-28`.** Both hosts that connected used
the `initialize` handshake and named `2025-11-25`, which is why that revision
is implemented as an adapter rather than dropped. The primary revision is
exercised only by the two project harnesses.

### What the hosts changed in this build

Running Cursor found one real defect, now fixed. Cursor rejected the whole
catalog because each tool's `outputSchema` was `{ "oneOf": [ success, error ] }`
with no top-level `type`. Its validator requires `type: "object"` before it
reads `oneOf`, and it failed the load with
`path: ["tools", 0, "outputSchema", "type"]`. Both branches of the union are
objects, so the union now says `"type": "object"` as well, and Cursor then
listed all 22 tools.
`mcp::catalog::tests::every_output_schema_admits_both_shapes` pins it.

### What stayed untested

- **The approval round trip in a third-party host.** Both connected hosts
  declare elicitation, but neither `claude mcp list` nor
  `cursor-agent mcp list-tools` calls a tool, and driving a tool call needs a
  model turn. **No claim is made that Claude Code or Cursor renders the
  approval form correctly.**
- **Codex's handshake**, for the reason above.
- **Every host's GUI.** Only the three command-line interfaces were run.
- **A real Canvas instance.** The identity used here points at a local mock
  over plain `http`, which a release build refuses (SPEC §11); these runs used
  a debug build with `CANVAS_TEST_ALLOW_HTTP=1`. That gate affects how the
  identity was created, not the protocol on the pipe.
- **Subscriptions.** Nothing in this release changes the resource list, so no
  host was ever sent a notification.

### Cleanup

The host runs created a scratch identity against a local mock and registered
the server with each host. All of it was removed: no keychain entry, no host
configuration entry, and no files under `~/.config/canvas-cli` or
`~/.local/share/canvas-cli`. Codex rewrote its own `config.toml` while adding
and removing the entry; the file parses to a semantically identical document.

---

## 3. Benchmark numbers

`cargo xtask bench --mcp --runs 3`, appended to `docs/bench.md` as
`## Agent surface (canvas mcp)`.

| Metric | p50 ms | p95 ms | Target p95 | Verdict |
|---|---:|---:|---:|---|
| warm `todo.list` round trip | 2.9 | 3.1 | 100 | ok |

3 timed calls after 5 warm-up calls on one long-lived connection, measured
from writing the request line to reading the response line, so the number
carries the process's own work and the pipe and no model time.

**Schema cost: 22 tools, 166 501 bytes, about 41 632 tokens per `tools/list`.**
The token column is one token per 4 bytes of UTF-8, stated as a rule of thumb
for JSON with English identifiers; the byte column is exact. Most of each row
is the output schema, which is the whole §7 envelope in both shapes with every
sub-schema inlined, because a host validator reads a tool definition on its
own. **This total is large enough to be worth an owner decision**, and the
generated section says so and names it as the number to beat if the catalog is
ever trimmed.

Round trips per workflow are also in the section, one row per skill workflow,
with the calls the harness cannot issue counted and explained.

---

## 4. The readings chosen

Where the report left something undefined, the reading below is the one that
exposes less and never dispatches a remote write without a recorded human
approval.

1. **Cache hints on a tool result.** The 2026-07-28 caching rules allow
   `ttlMs` and `cacheScope` only on `server/discover`, `tools/list`,
   `prompts/list`, `resources/list`, `resources/templates/list` and
   `resources/read`. A `tools/call` result therefore carries them in `_meta`
   under `dev.canvas-cli/cacheScope` and `dev.canvas-cli/ttlMs`.
   `resources/read` uses the real fields.
2. **`--mcp` adds a section** to `docs/bench.md` rather than replacing the
   report. Without the flag the document says the agent surface was not
   measured and names the flag, so a plain `bench` run cannot silently drop
   the §13 numbers.
3. **`subscriptions/listen` is declared** by accepting the resource-list
   category only. Returning nothing would leave the method unimplemented;
   nothing in this release changes that list, so a host's subscription is
   answered and stays quiet. Per-resource events are a later package.
4. **The consumer id is `mcp:<client name>`** from the per-request
   `clientInfo`, and `mcp` when a host does not name itself. It is recorded on
   the plan and on every approval handle, so the audit says who asked, and it
   is stable across the two halves of one round trip.
5. **`requestState` carries only `{ plan_id, handle }`.** The rmcp
   documentation warns that a client echoes the state verbatim, so the state
   is treated as untrusted: `approve` validates the random server-issued
   handle against the stored row, which is what makes an echoed or forged
   state unable to approve anything. The retry must echo the handle in
   `inputResponses`; a missing handle is a protocol error, and a wrong one is
   a domain refusal.
6. **Elicitation counts as declared** when `elicitation` is present and not
   URL-only. An empty object is the 2025-06-18 form-mode declaration, and a
   host that declares URL mode alone cannot answer this server.
7. **Decline and cancel both invalidate the plan** and report exit 11 with
   code `cancelled`, which is what `canvas submit` reports for "No". §7 has no
   `cancelled` outcome, so the outcome of that envelope stays `error`.
8. **An executed plan replays locally**, without a client, so a lost response
   is recoverable when the network is gone.
9. **`submission.execute` is `idempotentHint: true`**, because a plan admits
   at most one journal, so a second call returns the journal the first one
   created. `submission.prepare` is `idempotentHint: false` and not read-only,
   because each call freezes a new plan.
10. **`submission.prepare` keeps the organize effect**, and
    `submission.execute` is the only remote write in the catalog. A test pins
    that it is the only one.
11. **`--text -` is refused** on this surface, because stdin carries the
    protocol. A text entry must name a file.
12. **`plan@1` gained a typed schema.** Its registry fixture carries `text`,
    `url` and `journal_id` as `null`, which an inferred schema reads as
    null-only, so the tool's output schema would have been wrong.

---

## 5. Behaviour changes

These change what an existing caller sees. Each one is a §7 or §14
consistency fix, or an interoperability fix a real host forced.

1. **A plan refusal now reports `outcome: refused`.** It reported
   `outcome: error` with `exit: 8` before, which §7 does not allow: exit 8 is
   a refusal, and a host reads `outcome` first. This affects `canvas submit`
   as well as the agent surface.
2. **`submit@1` carries a new `replayed` boolean**, always present and `false`
   everywhere except a replay. Adding a field keeps `@1`. Four snapshots and
   the registry fixture were regenerated.
3. **`submission.execute` on a plan that already has a journal** returns that
   journal's `submit@1` envelope with the journal's own `outcome` and `exit`
   plus `replayed: true`, per SPEC §19 item 17. It never creates a second
   journal and never returns a bare refused-8 without a reason. The human
   `canvas submit` cannot reach that path: it approves the plan it just froze,
   so an executed plan is still a refusal there.
4. **The envelope union in every `schema@1` document declares
   `type: "object"`.** Both branches are objects, so the union says so. This
   is what Cursor's validator requires, and it changes the output of
   `canvas schema` for every command by one line.
5. **The forbidden-surface test no longer greps for `exec`**, which matched
   the legitimate `submission.execute`. It greps for `shell`, `eval` and
   `spawn` instead, and the write surface is pinned by effect: exactly one
   tool may reach Canvas.
6. **`canvas open` gained a `Launch` parameter** so the agent surface can
   resolve a URL without opening a browser. The CLI passes `Launch::Yes` and
   behaves as before.

---

## 6. Dependencies for Appendix A

Two new direct dependencies, both pinned with `=` as Appendix A requires.
They are declared in the workspace `[workspace.dependencies]` and used by
`canvas-cli` only.

| Crate | Version | Why | Features |
|---|---|---|---|
| `rmcp` | `=3.2.0` | The Model Context Protocol server and its model types. | `server`, `client`, `macros`, `elicitation`, `transport-io`, `transport-async-rw`, `schemars`, `local`; `default-features = false` |
| `schemars` | `=1.2.2` | JSON Schema generation for `canvas schema` and for every tool's input and output schema. | default |

Notes for the appendix:

- The `local` feature of `rmcp` makes `MaybeSend` and `MaybeSendFuture` no-ops
  and switches the SDK to `spawn_local`. The command cores are `!Send`, so the
  service runs on one thread inside a `LocalSet`. Without this feature the
  handlers would not compile.
- The `client` feature is needed for the model types the server shares with a
  client, not to speak as a client.
- `cargo deny check` passes: advisories, bans, licenses and sources are all
  clean with these two crates and their transitives.

`Cargo.lock` gains 23 packages in total. Beyond the two above, the
transitives are: `android_system_properties`, `chrono`, `darling`,
`darling_core`, `darling_macro`, `dyn-clone`, `iana-time-zone`,
`iana-time-zone-haiku`, `ident_case`, `pastey`, `ref-cast`, `ref-cast-impl`,
`rmcp-macros`, `schemars_derive`, `serde_derive_internals`, `tokio-stream`,
`windows-core`, `windows-implement`, `windows-interface`, `windows-result`,
`windows-strings`.

`chrono` arrives through `rmcp`. It is a timestamp library the workspace does
not otherwise use, because §7 timestamps go through `jiff`. It is worth an
owner decision whether that duplication is acceptable, or whether it should be
recorded as an accepted exception in Appendix A.

---

## 7. Files this package owns

- `crates/canvas-cli/src/mcp/` — `mod.rs`, `server.rs`, `catalog.rs`,
  `resources.rs`, `result.rs`
- `crates/canvas-cli/src/commands/schema.rs` and the handler extraction across
  `crates/canvas-cli/src/commands/`
- `crates/canvas-cli/src/output/json_schema.rs` and the `schema@1` registry
  entry
- `crates/canvas-cli/tests/mcp.rs`, `crates/canvas-cli/tests/skill.rs`,
  `crates/canvas-cli/tests/schema_cmd.rs`
- `skill/canvas-cli/`
- `xtask/src/bench_mcp.rs` and the `--mcp` extension in `xtask/src/bench.rs`
- `docs/agent-hosts.md`, and the appended section of `docs/bench.md`
```