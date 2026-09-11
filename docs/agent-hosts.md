# Agent hosts

`canvas mcp` serves the Model Context Protocol on stdin and stdout. This file
records which hosts were **actually run against this build on this machine**,
what each one negotiated, and what stayed untested. A host that is not in the
table below was not exercised, and a row never claims more than the session
log shows.

Recorded 2026-09-10 on macOS 26.6.2, arm64, from `lane/w2`, against a
22-tool catalog. The catalog grew to 43 tools after these runs and was
narrowed back to the same 22 reads later the same day (SPEC §19 item 48);
the tool counts below therefore match the current build, but **no row was
re-run against it**.

## What ran

`input_required` has no column any more: the server no longer implements
that round trip, because no tool in the catalog acts (SPEC §21.2).

| Host | Version | Connected | Revision | `tools/list` | `resources/list` | Elicitation declared |
|---|---|---|---|---|---|---|
| Claude Code (CLI) | 2.1.267 | yes | `2025-11-25` | 22 tools | not requested | yes, `elicitation: {}` |
| Cursor (`cursor-agent`) | 2026.09.08-6caf4ff (client id `Cursor 1.0.0`) | yes | `2025-11-25` | 22 tools | 2 resources | yes, `elicitation: { form: {} }` |
| Codex CLI | 0.153.4 | not exercised | — | — | — | — |
| Project harness (`crates/canvas-cli/tests/mcp.rs`) | this build | yes | `2026-07-28` and `2025-11-25` | 22 tools | 2 resources + 2 templates | yes and no, both cases |
| Project harness (`cargo xtask bench --mcp`) | this build | yes | `2026-07-28` | 22 tools | not requested | no |

A host that declares elicitation is never asked for one. The declaration is
recorded because it is what the host sent, not because the server uses it.

## How each row was produced

Every host launched the same wrapper, which `tee`s both directions of the
stdio session to a file, so each row is read off a session log rather than
inferred from a screen.

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
  exercises both protocol revisions, the resource namespace, and the
  subscription cursor. It also pins that every tool on the wire is a read,
  that each of the 21 removed names is `METHOD_NOT_FOUND`, and that a request
  carrying a `requestState` is refused.
  `cargo xtask bench --mcp` is a second hand-written client, used for the
  numbers in [`bench.md`](bench.md).

## What this says about the two revisions

**No third-party host asked for `2026-07-28`.** Both hosts that connected used
the `initialize` handshake and named `2025-11-25`, which is why that revision
is implemented as an adapter rather than dropped. The primary revision — no
handshake, a per-request `_meta`, `server/discover` — is exercised only by the
two project harnesses in the table.

## What the hosts changed in this build

Running the hosts found one real defect, now fixed:

- **Cursor rejected the whole catalog** because each tool's `outputSchema` was
  `{ "oneOf": [ success, error ] }` with no top-level `type`. Its validator
  requires `type: "object"` before it reads `oneOf`, and it failed the load
  with `path: ["tools", 0, "outputSchema", "type"]`. Both branches of the
  union are objects, so the union now says `"type": "object"` as well. After
  that, Cursor listed all 22 tools.
  `mcp::catalog::tests::every_output_schema_admits_both_shapes` pins it, and
  still does over the narrowed catalog.

## What stayed untested

- **Any tool call in a third-party host.** Neither `claude mcp list` nor
  `cursor-agent mcp list-tools` calls a tool, and driving a tool call needs a
  model turn. Both hosts loaded the catalog; **no claim is made that either
  one renders a result correctly.**
- **Codex's handshake**, for the reason above.
- **Every row against the current build.** The two third-party runs predate
  the M9 narrowing. They are kept because the catalog they loaded is the same
  size and holds the same 22 reads, and because the defect one of them found
  is still pinned by a test — but they were not repeated.
- **Every host's GUI.** Only the three command-line interfaces were run.
  Claude Desktop, the Cursor editor, and the Codex IDE extension were not.
- **A real Canvas instance.** The identity used here points at a local mock
  over plain `http`, which a release build refuses (SPEC §11); these runs used
  a debug build with `CANVAS_TEST_ALLOW_HTTP=1`. That gate affects how the
  identity was created, not the protocol on the pipe.
- **Subscriptions.** The server declares `subscriptions/listen` and accepts
  the resource-list category only. Nothing in this release changes that list,
  so no host was ever sent a notification.

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
