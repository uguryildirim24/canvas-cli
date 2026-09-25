# Agent hosts

`canvas mcp` serves the Model Context Protocol on stdin and stdout. This file
records which hosts were **actually run against this build on this machine**,
what each one negotiated, and what stayed untested. A host that is not in the
table below was not exercised, and a row never claims more than the session
log shows.

## What the surface is

One tool, `getclitools`, and nothing else. It performs nothing: it returns the
`canvas` command reference and the agent runs the commands itself (SPEC §21.2,
§19 item 50). There are no resources, no resource templates, and no
subscriptions, so `initialize` and `server/discover` advertise `tools` and
nothing more.

That makes the host matrix short. A host has three things to get right — the
handshake, one `tools/list`, and one `tools/call` — and there is no per-tool
schema for a validator to reject.

## What ran

Recorded 2026-09-10 on macOS 26.6.2, arm64, against a 22-tool catalog. **No third-party row was re-run against the one-tool
build.** The rows are kept because the handshake they exercised is unchanged
and because one of them found a defect that is still pinned by a test; what
they loaded is not what this build serves.

| Host | Version | Connected | Revision | `tools/list` | `resources/list` | Elicitation declared |
|---|---|---|---|---|---|---|
| Claude Code (CLI) | 2.1.267 | yes | `2025-11-25` | 22 tools (then) | not requested | yes, `elicitation: {}` |
| Cursor (`cursor-agent`) | 2026.09.08-6caf4ff (client id `Cursor 1.0.0`) | yes | `2025-11-25` | 22 tools (then) | 2 resources (then) | yes, `elicitation: { form: {} }` |
| Codex CLI | 0.153.4 | not exercised | — | — | — | — |
| Project harness (`crates/canvas-cli/tests/mcp.rs`) | this build | yes | `2026-07-28` and `2025-11-25` | 1 tool | empty | yes and no, both cases |
| Project harness (`cargo xtask bench --mcp`) | this build | yes | `2026-07-28` | 1 tool | not requested | no |

A host that declares elicitation is never asked for one. Nothing on this
surface asks a person for a decision, and a request that carries a
`requestState` is refused rather than run. The declaration is recorded because
it is what the host sent.

## How each row was produced

Every third-party host launched the same wrapper, which `tee`s both directions
of the stdio session to a file, so each row is read off a session log rather
than inferred from a screen.

- **Claude Code.** `claude mcp add … -- <wrapper>` in a scratch directory,
  then `claude mcp list`, which health-checks the server. The log shows
  `initialize` with `protocolVersion: 2025-11-25` and
  `capabilities.elicitation: {}`, then `notifications/initialized` and
  `tools/list`. The entry was removed afterwards.
- **Cursor.** A `.cursor/mcp.json` in a scratch workspace, then
  `cursor-agent mcp enable canvas-hostmatrix` and
  `cursor-agent mcp list-tools canvas-hostmatrix`. The log shows `initialize`
  with `protocolVersion: 2025-11-25` and
  `capabilities.elicitation: { form: {} }`, then `tools/list` and
  `resources/list`. The workspace was deleted afterwards.
- **Codex CLI.** `codex mcp add` accepted the stdio configuration and
  `codex mcp list` and `codex mcp get` showed it enabled, but neither command
  launches the server: no process started and no session log exists. The
  configuration was removed afterwards. **Nothing about Codex's handshake is
  claimed here.**
- **The project harnesses.** `crates/canvas-cli/tests/mcp.rs` speaks
  hand-written JSON-RPC over a real pipe. It is the only client here that
  exercises both protocol revisions. It pins that exactly one tool is served,
  that its answer is the `canvas` command reference — compared against the
  `canvas schema --list` the same build prints — that each of the 43 names the
  server has served at one time or another is `METHOD_NOT_FOUND`, and that
  `resources/read`, `subscriptions/listen`, an unknown argument, and a
  `requestState` are all refused. `cargo xtask bench --mcp` is a second
  hand-written client, used for the numbers in [`bench.md`](bench.md).

## What this says about the two revisions

**No third-party host asked for `2026-07-28`.** Both hosts that connected used
the `initialize` handshake and named `2025-11-25`, which is why that revision
is implemented as an adapter rather than dropped. The primary revision — no
handshake, a per-request `_meta`, `server/discover` — is exercised only by the
two project harnesses in the table.

## What the hosts changed in this build

Running the hosts found one real defect:

- **Cursor rejected the whole catalog** because each tool's `outputSchema` was
  `{ "oneOf": [ success, error ] }` with no top-level `type`. Its validator
  requires `type: "object"` before it reads `oneOf`, and it failed the load
  with `path: ["tools", 0, "outputSchema", "type"]`. Both branches of the
  union are objects, so the union said `"type": "object"` as well, and Cursor
  then listed all 22 tools.

  **This build has no `outputSchema` at all.** `getclitools` answers with one
  Markdown document, not a §7 envelope, so there is nothing for a validator to
  read. The rule the defect taught — a union must declare its type — still
  holds wherever the CLI generates one, and
  `crates/canvas-cli/tests/mcp.rs` pins that the one tool declares no output
  schema, which is the stronger version of the same protection.

## What stayed untested

- **Every third-party row against the current build.** The two runs predate
  both the M9 narrowing and this one. They loaded a catalog that no longer
  exists.
- **Any tool call in a third-party host.** Neither `claude mcp list` nor
  `cursor-agent mcp list-tools` calls a tool, and driving a tool call needs a
  model turn. **No claim is made that either host renders the reference
  correctly**, or that a model reading it goes on to run the commands.
- **Codex's handshake**, for the reason above.
- **Every host's GUI.** Only the three command-line interfaces were run.
  Claude Desktop, the Cursor editor, and the Codex IDE extension were not.
- **A real Canvas instance.** The identity used here points at a local mock
  over plain `http`, which a release build refuses (SPEC §11); these runs used
  a debug build with `CANVAS_TEST_ALLOW_HTTP=1`. That gate affects how the
  identity was created, not the protocol on the pipe. `getclitools` reaches
  no Canvas instance at all: it reads the binary's own command tree.

## Setting a host up yourself

One instance serves one identity. Pick it with `--profile`, or leave it out
for the default profile.

```json
{
  "mcpServers": {
    "canvas": { "command": "canvas", "args": ["--profile", "default", "mcp"] }
  }
}
```

- **Claude Code:** `claude mcp add canvas -- canvas --profile default mcp`,
  then `claude mcp list` to health-check it.
- **Codex CLI:** `codex mcp add canvas -- canvas --profile default mcp`.
- **Cursor:** the JSON above in `.cursor/mcp.json` or `~/.cursor/mcp.json`,
  then `cursor-agent mcp enable canvas`.

The host also needs a way to run `canvas` itself — a shell, or whatever
execution capability it has — because that is where every read and every write
happens. A host with no shell can still call `getclitools`, but it can only
show the user the command line to run.

The shipped skill in [`../skill/canvas-cli/SKILL.md`](../skill/canvas-cli/SKILL.md)
carries the same setup plus the workflows, the envelope reading order, and the
exit-code table.

## If a host will not connect

- `canvas mcp` refuses to start without an identity and exits 3. Run
  `canvas auth status` first.
- One instance is bound to one identity **and** one identity generation. It
  stops with exit 13 when the identity is replaced or removed, and the host
  must restart it.
- The server writes one JSON-RPC message per line on stdout and nothing else.
  A wrapper script that prints anything to stdout breaks the transport; send
  wrapper output to stderr or to a file.
