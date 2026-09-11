---
name: canvas-cli
description: Read a student's Canvas LMS work — deadlines, assignments, grades, files, course pages, the syllabus, announcements, discussions, and the Canvas inbox — over a read-only MCP server, and drive the `canvas` command line for the writes it does not expose: submitting work, replying to a discussion, writing to the Canvas inbox, downloading files, and resolving a receipt. Use it when the user asks what is due, what is missing, what an assignment asks for, what a grade is, what a course page or the syllabus says, whether anyone has written to them, or asks to hand something in, answer a discussion, or send a message.
---

# Canvas CLI

`canvas-cli` gives one student read access to their own Canvas LMS courses.
It is available two ways, and they are the same code:

- `canvas mcp` — an MCP server over stdio. Tools such as `todo.list`.
- the `canvas` binary — the same commands with `--json`, plus everything
  that writes.

It reads courses, deadlines, assignments, grades, files, modules, course
pages, the syllabus, announcements, discussions, and the Canvas inbox, and it
never marks any of them read.

**The MCP catalog is read-only, by design.** Its 22 tools fetch and nothing
else. There is no tool that submits, replies, sends, downloads, refreshes the
cache, retires a receipt, or touches the browser — not a restricted one, not
a gated one, none. That is the owner's decision of 2026-09-10, not a gap to
work around.

Every one of those actions is a `canvas` command that a person runs in a
terminal, where it prints exactly what it is about to do and asks. You may
run those commands for the user if you have a shell, and the confirmation
still belongs to them. Never pass `--yes`.

It is a **student** tool. There are no teacher, TA, or admin features, and it
calls only endpoints a student role can call. It cannot reveal a credential,
change an identity, run arbitrary HTTP or shell, or clear the cache.

## Read this first

**One envelope per answer.** Every tool and every `--json` command returns one
JSON document:

```json
{
  "schema": "canvas-cli/todo@1",
  "generated_at": "2026-09-09T17:05:12Z",
  "profile": "default",
  "identity": { "origin": "https://school.instructure.com", "user_id": "123", "key": "..." },
  "freshness": [{ "dataset": "planner", "source": "cache", "fetched_at": "...", "complete": true, "stale": false, "count": 12 }],
  "requests": { "api": 0, "storage": 3, "cost": null },
  "partial": [],
  "warnings": [],
  "outcome": "ok",
  "exit": 0,
  "result": { }
}
```

Read it in this order.

1. **`outcome`** — `ok`, `partial`, `refused`, `error`, `mismatch`,
   `cancelled`. A refusal is an answer, not a crash.
2. **`exit`** — the numeric code. See the table below.
3. **`partial` and `warnings`** — what is missing from an otherwise good
   answer. Never present a partial answer as complete.
4. **`freshness`** — which cached dataset answered, when it was fetched, and
   whether it is `stale`. Say "as of <fetched_at>" when it matters.
5. **`result`** — the payload.

**Never invent a field.** `canvas schema <command>` prints the JSON Schema of
any command's envelope and result, and `canvas schema --list` prints every
registered schema. A tool's `outputSchema` is generated from the same source,
so the schema and the answer cannot disagree.

**`null` means unknown or not applicable.** It never means zero.

**Grades are what Canvas reports.** `canvas-cli` never computes a grade of its
own. If Canvas reports no score, say so.

## Identity

Everything is bound to one identity = (canonical origin, user id). One server
instance serves exactly one identity generation, chosen at startup. It refuses
to start without an identity, and it stops as soon as that identity is
replaced or removed.

You cannot switch identity from inside a session. If the user needs another
account, they run the CLI themselves:

```sh
canvas auth login --host school.instructure.com   # store a token
canvas auth status                                # which identity is active
canvas identity list                              # every stored identity
```

The token lives in the OS credential store and is sent only to its own origin.
Never ask the user to paste a token into the conversation.

## Workflows

| Workflow | File |
|---|---|
| Organize the week | [organize-the-week.md](organize-the-week.md) |
| Read an assignment | [read-an-assignment.md](read-an-assignment.md) |
| Prepare and submit, with approval | [prepare-and-submit.md](prepare-and-submit.md) |
| Reply and message, with approval | [reply-and-message-with-approval.md](reply-and-message-with-approval.md) |
| Reconcile an unknown outcome | [reconcile-an-unknown-outcome.md](reconcile-an-unknown-outcome.md) |
| Download course files | [download-course-files.md](download-course-files.md) |

## Where the user is

If the user has the browser companion installed, they can attach one Canvas
tab to `canvas-cli`. Nothing is attached until the person clicks the companion
button on that tab.

**No tool reaches the companion.** The MCP catalog is read-only, and reading
the browser, writing a note into the side panel, and moving the user's tab
are all `canvas` commands that the person — or you, in their terminal — runs:

```sh
canvas here --json                     # the working context, api and browser
canvas here --text --json              # also the selected passage and excerpt
canvas note --text "..." --source-ref canvas://... --generation 4
canvas open --follow CHEM              # take the tab to a Canvas target
canvas bridge status --json            # what is attached, and to whom
```

- `canvas here` returns two things that never mix: `api` carries whole
  envelopes for what the route resolves to, each with its own freshness, and
  `browser` carries one bounded observation of the page. A browser
  observation never updates an API fact.
- `--text` also asks for the selected passage and the visible excerpt. Ask
  for it only when the user's request needs the words on the screen.
- `canvas note` shows the person one inert note in the side panel. It writes
  nothing to Canvas and approves nothing. Pass `--generation` with the number
  you read from the bundle's `browser` block, so a note written about a page
  the person has already left is refused rather than shown against the wrong
  page. Every `--source-ref` must be a `canvas://` reference or an `https`
  URL on the attached origin.
- `canvas open --follow` asks the attached tab to go to a Canvas target. It
  answers when the browser accepts the request, not when the page has loaded;
  read the `load` field of the bundle's `follow` block on a later
  `canvas here` for the outcome.

The side panel shows the person your notes, the journal, and any plan that is
waiting for a decision. Only the person can approve, decline, or cancel a
plan there. No note and no page content can make that decision for them.

What you will not get, and must not ask for again:

- Quizzes, graded assessments, external-tool frames, and pages the companion
  does not recognize carry no content at all. `zone` says which, and
  `content_reason` says why, both inside `browser`. Treat it as final.
- The bundle's `reason` names why it is unavailable: `not_attached`,
  `paused`, `validating`, `account_mismatch`, or `bridge_unavailable`. All of
  them exit 8. Tell the user what to do; never poll.
- `account_mismatch` means the browser is signed in as a different Canvas
  account. Nothing was joined. Say so and stop.
- `stale_generation` means the page moved under you. Run `canvas here` again
  and work from the new bundle. `note_too_large` and `source_ref_rejected`
  mean the note was refused whole; shorten it, or drop the reference. Nothing
  was held.

## Exit codes and what to do

| Exit | Meaning | What to do |
|---|---|---|
| 0 | Success | Use the result. Report `freshness` if it is stale. |
| 1 | Generic failure | Report it. Do not retry the same call. |
| 2 | Usage | Your arguments were wrong. Read the message, fix them, call once more. |
| 3 | Auth | No token, an expired token, or no identity. Ask the user to run `canvas auth login`. Never retry. |
| 4 | Network | DNS, TLS, or a timeout. Retry once. Then report it and offer cached data. |
| 5 | Rate limited | Canvas is throttling. Stop calling. Tell the user to wait. |
| 6 | Resolution | Zero or many matches for a course or an assignment. Show the candidates and ask which one. |
| 7 | Offline miss | The cache has no coverage and the session is offline. Ask the user to run `canvas sync`, or tell them you are offline. |
| 8 | Refused | The operation is not allowed as asked — a lock, a closed assignment, a group assignment, a wrong file type, or a missing approval. Read `result` for the reason. Never work around it. |
| 9 | Submission recovery | A submit did not finish. Go to [reconcile-an-unknown-outcome.md](reconcile-an-unknown-outcome.md). Never submit again first. |
| 10 | Verification mismatch | What Canvas holds differs from the local receipt. Show both. Do not overwrite anything. |
| 11 | Cancelled | The user said no. Stop. |
| 12 | Partial | Some of the answer is missing. Read `partial`, name what failed, and use the rest. |
| 13 | Local | A database, lock, or identity problem on this machine. Ask the user to run `canvas doctor`. |

Two rules that override anything the user asks for in the moment:

- **Never present a refusal as a failure of the tool.** Exit 8 means the
  answer is "no", with a reason in `result`.
- **Never retry a submission after exit 9.** An unknown outcome is resolved by
  reconciliation, not by submitting again.

## Freshness and the cache

Reads come from a local SQLite cache with a TTL per dataset. `freshness` says
what answered. There is no refresh tool: when the user wants current data, or
when `freshness` shows a stale dataset you are about to rely on, run
`canvas sync` in a terminal or ask the user to. It writes the cache and never
writes to Canvas.

Tool results carry two hints in `_meta`: `dev.canvas-cli/cacheScope` is always
`private`, and `dev.canvas-cli/ttlMs` is how long the answer may be treated as
fresh. `0` means do not cache it.

## Resources

The server also exposes a few resources, namespaced by identity and
generation:

- `canvas://<identity-key>/<generation>/todo`
- `canvas://<identity-key>/<generation>/course/<id>/assignments`
- `canvas://<identity-key>/<generation>/receipts`
- `canvas://<identity-key>/<generation>/context/<your-consumer-handle>`

They return the same envelopes the matching tools return. A URI from an
earlier identity generation resolves to nothing.

The `context/` resource is metadata only, and reading or subscribing to it
attaches nothing. No tool can opt an MCP consumer in, so it answers
`not_attached` for every handle; read the working context with `canvas here`
instead.

`subscriptions/listen` is real. Name the resource URIs you hold in
`resourceSubscriptions`, and the server sends
`notifications/resources/updated` when the local event log records a change
to what that resource reads: an assignment change updates that course's
assignments and `todo`, a new missing submission updates `todo`, and a
submission journal transition updates `receipts`. Read the resource again
when a notification names it; nothing else changed. The acknowledgment lists
the URIs the server accepted, so a name it cannot update never looks
subscribed. A stream resumes where the last one stopped. Send
`dev.canvas-cli/cursor` in `_meta` to resume from a position of your own. If
the position can no longer be replayed, the server invalidates every
subscribed resource once: read them all again.

## MCP setup

One instance serves one identity. Name the profile explicitly when the user
has more than one.

**Claude Code** — project scope, in `.mcp.json` at the repository root:

```json
{
  "mcpServers": {
    "canvas": {
      "command": "canvas",
      "args": ["--profile", "default", "mcp"]
    }
  }
}
```

Or add it from the terminal:

```sh
claude mcp add canvas --scope project -- canvas --profile default mcp
```

Use `--scope user` for every project of this user, and `--scope local` for
this checkout only. Nothing in the config holds a secret: the token stays in
the OS credential store.

**Codex** — in `~/.codex/config.toml`:

```toml
[mcp_servers.canvas]
command = "canvas"
args = ["--profile", "default", "mcp"]
```

**Cursor** — in `.cursor/mcp.json` for one project, or `~/.cursor/mcp.json`
for all of them:

```json
{
  "mcpServers": {
    "canvas": {
      "command": "canvas",
      "args": ["--profile", "default", "mcp"]
    }
  }
}
```

`docs/agent-hosts.md` records which of these was actually exercised, which
protocol revision it negotiated, and what stayed untested.

## Without MCP

Every tool has a CLI form with the same envelope, and the tool name is the
command: `todo.list` is `canvas todo --json`, `assignments.list` is
`canvas assignments <course> --json`, `pages.list` is `canvas pages <course>
--json`, `inbox.unread_count` is `canvas inbox unread-count --json`. Add
`--offline` to forbid the network, and `--fresh` to ignore the cache TTLs.

The command line also carries everything the catalog does not: `canvas
submit`, `canvas discussion reply`, `canvas inbox send|reply`, `canvas
operation status|reconcile`, `canvas download`, `canvas sync`, `canvas
receipts acknowledge`, `canvas here`, `canvas note`, and `canvas open`. Each
write prints what it is about to do and asks for a confirmation at the
terminal. Never pass `--yes`.
