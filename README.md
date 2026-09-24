# canvas-cli

`canvas-cli` is a Rust command-line client for Canvas LMS (Instructure), built
for **students only**. One binary, `canvas`, gives a student their deadlines,
Canvas-reported grades, course files, announcements, and a way to submit work
with a durable journal and a local receipt, without opening the Canvas web app.
It is fast because it keeps a local SQLite cache, safe because the API token
lives in the OS credential store and is sent only to its own origin, and
scriptable because every data command has a defined `--json` schema. It has no
teacher, TA, or admin features, and it calls only endpoints a student role can
call.

## Why

Canvas is a web app, and the things a student needs from it most often — what
is due, what is missing, what an assignment actually asks for, what a grade is
— are each three clicks and a page load away. A terminal client makes them one
command, makes them scriptable, and makes them available to a coding agent
without handing that agent a browser session or a password.

## Status

**Working, in daily use, not released.** Version `0.1.0`, no tag, no published
package. The whole command surface below runs: 923 tests pass, and CI gates
every commit on `fmt`, `clippy -D warnings`, the test suite, `cargo deny`, and
an MSRV check.

What "not released" means in practice:

- There is no Homebrew tap and no crates.io release yet, so **building from
  source is the only install route.** `docs/release.md` is the runbook for
  making the other routes live.
- The browser companion in `extension/` has been exercised end to end against
  a real native-messaging host process, but its Chrome-side flows have not
  been run in a real browser. `docs/companion.md` says exactly what was and
  was not tried.
- `docs/SPEC.md` §19 lists the design questions that are still open.

`docs/SPEC.md` is the contract this code implements. This README is the short
version.

## Install

```sh
git clone https://github.com/uguryildirim24/canvas-cli
cd canvas-cli
cargo build --release        # binary at target/release/canvas
```

Rust 1.88 or newer. Put `target/release/canvas` on your `PATH`, or run
`cargo install --path crates/canvas-cli`.

To run the same gates CI runs:

```sh
just check                   # fmt, clippy, tests, cargo-deny
just msrv                    # the 1.88 check CI runs alongside them
```

Once a release exists, `brew install uguryildirim24/homebrew-tap/canvas-lms-cli`
and `cargo binstall canvas-lms-cli` become the supported routes. Direct archive
downloads on macOS will not be, until the binaries are signed and notarized —
so there will be no `curl | sh` installer.

## First run

```sh
canvas auth login --host canvas.example.edu
```

`auth login` asks for a **personal access token**. Create one in Canvas at
`Account → Settings → Approved Integrations → + New Access Token`, or let
`auth login` open `https://<your-host>/profile/settings` for you. The token is
read from a hidden prompt, from `--token-stdin`, or from `CANVAS_TOKEN`, and
the command prints which source it used.

`canvas-cli` then calls `GET /api/v1/users/self` and stores the token in the OS
credential store (macOS Keychain, Windows Credential Manager, Secret Service on
Linux) under the identity it just confirmed. Every cache row, alias, journal,
receipt, and download is bound to that identity = (canonical origin, user ID).
Where no credential store is available, the token goes to
`~/.config/canvas-cli/credentials.toml` with mode `0600`.

Check the result with `canvas auth status`, and check the whole local setup
with `canvas doctor`.

## A short example

A week, an assignment, and a hand-in:

```console
$ canvas todo --days 7
BIO-310  Lab report 4                 due Thu 23:59  (in 2d 4h)
CHEM-201 Problem set 7                due Fri 17:00  (in 3d 21h)
CHEM-201 Reading response 3           MISSING        (overdue 3d)

$ canvas assignment chem "problem set 7"
CHEM-201 · Problem set 7 · 20 points · due 2026-09-26 17:00
Submit one PDF. Show your work for every equilibrium calculation.
Your submission: none yet.

$ canvas submit chem "problem set 7" --file ps7.pdf
about to submit 1 file to CHEM-201 · Problem set 7
  ps7.pdf  412 KB  sha256 9f2a…c41d
proceed? [y/N] y
submitted · attempt 1 · receipt r-01K6QX3
```

Every one of those takes `--json` and prints a single documented envelope, so
the same three steps script cleanly:

```sh
canvas todo --days 7 --json | jq -r '.result.items[] | select(.missing) | .title'
```

## Global flags

| Flag | Meaning |
|---|---|
| `--json` | Machine-readable output, one JSON document per invocation. |
| `--color auto\|always\|never` | Honors `NO_COLOR`, `CLICOLOR_FORCE`, and TTY detection. |
| `--profile NAME` | Select a named profile, and through it an identity. |
| `--fresh` | Ignore cache TTLs. Conflicts with `--offline`. |
| `--offline` | Never touch the network. |
| `-q`, `--quiet` | No progress, no info logs. |
| `-v`, `--verbose` | Debug logs on stderr, with tokens redacted. |

## Commands

Every command below runs in this build.

| Command | What it does |
|---|---|
| `canvas auth login` | Store a personal access token for a Canvas host. `--host`, `--token-stdin`, `--replace`. |
| `canvas auth status` | Show the active profile, identity, and credential backend. |
| `canvas auth logout` | Remove the active token from the credential store. |
| `canvas auth token` | Print token metadata, or the secret itself with `--reveal`. |
| `canvas identity list` | List stored identities with their profiles and on-disk sizes. |
| `canvas identity remove` | Delete an identity: credentials, then data, then profiles. |
| `canvas courses` | List your courses. `--all`, `--term`, `--favorites`. |
| `canvas course` | Show one course: term, teachers, and reported scores. |
| `canvas todo` | What is due and what is missing. `--days`, `--all`, `--missing`, `--course`. |
| `canvas assignments` | List a course's assignments. `--bucket`, `--search`. |
| `canvas assignment` | Show one assignment prompt, dates, and your submission. |
| `canvas submit` | Submit files, text, HTML, or a URL, journaled and receipted. |
| `canvas submission` | Show your submission for an assignment. `--history`. |
| `canvas submission verify` | Re-check a receipt against what Canvas reports now. |
| `canvas submission reconcile` | Resolve a journal left unfinished by an interrupted submit. |
| `canvas receipts list` | List local submission receipts. `--course`, `--state`. |
| `canvas receipts show` | Show one receipt or journal in full. |
| `canvas receipts export` | Write a receipt to a JSON file. `--out`. |
| `canvas receipts acknowledge` | Accept a journal whose outcome stays unknown. |
| `canvas grades` | Scores as Canvas reports them. `--period current\|all\|ID`. |
| `canvas files` | List a course's files. `--tree`, `--search`. |
| `canvas download` | Download course files per module, never clobbering. `--dest`, `--module`, `--file`, `--jobs`, `--dry-run`, `--force`, `--verify`. |
| `canvas modules` | List a course's modules. `--items`. |
| `canvas announcements` | Recent announcements. `--since`, `--unread`. |
| `canvas announcement` | Show one announcement as Markdown. |
| `canvas pages` | List a course's pages. `--unpublished`. |
| `canvas page` | Show one page as Markdown, with what the Markdown cannot show. |
| `canvas syllabus` | Show the course syllabus as Markdown, with its file links. |
| `canvas discussions` | List a course's discussion topics. `--unread`. |
| `canvas discussion` | Show one discussion. `--replies`, `--page`. |
| `canvas discussion reply` | Reply to a discussion topic. Needs an approval. `--to`, `--text`, `--text-file`, `--yes`. |
| `canvas inbox` | List conversations. `--scope inbox\|unread\|sent\|archived`. |
| `canvas inbox show` | Show one conversation and its messages. |
| `canvas inbox unread-count` | Show the unread conversation count. |
| `canvas inbox send` | Send a new conversation. Needs an approval. `--to`, `--subject`, `--text`, `--text-file`, `--attach`, `--yes`. |
| `canvas inbox reply` | Add a message to a conversation. Needs an approval. `--text`, `--text-file`, `--attach`, `--yes`. |
| `canvas quizzes` | List a course's Classic Quizzes. |
| `canvas quiz` | Show one quiz: limits, attempts, and rules. |
| `canvas quiz questions` | Read the live session's questions. Starts a session first, with a confirmation. `--access-code`, `--yes`. |
| `canvas quiz submit` | Answer every question and turn the quiz in. Needs an approval. `--answers`, `--access-code`, `--yes`. |
| `canvas new-quizzes` | List a course's New Quizzes: due, attempts, time limit. Taking one stays in the browser. |
| `canvas new-quiz` | Show one New Quiz: its instructions and taking rules. |
| `canvas operation status` | Show one write operation and read its thread back. |
| `canvas operation reconcile` | Resolve a write operation left unfinished. `--assume-not-posted`. |
| `canvas calendar` | Calendar events. `--days`, `--course`, `--ics`, `--alarm`. |
| `canvas open` | Open a course, or any Canvas URL, in the browser. `--follow` navigates the attached tab instead; `--attachment`. |
| `canvas open assignment` | Open an assignment page in the browser. |
| `canvas open file` | Open a file by id in the browser. |
| `canvas open announcement` | Open an announcement page in the browser. |
| `canvas bridge install` | Write the Chrome native-messaging host manifest for this user. `--extension-id`, `--browser`. |
| `canvas bridge host` | Speak Chrome native messaging on stdin and stdout. Chrome starts it; you do not. |
| `canvas bridge status` | Report the host manifest, the broker owner, and the attachment. |
| `canvas bridge detach` | Ask the live broker to drop the attachment. `--attachment`. |
| `canvas here` | Show the attached Canvas page as a context bundle. `--attachment`, `--text`. |
| `canvas note` | Show one inert note in the companion's side panel. `--text`, `--source-ref`, `--attachment`, `--generation`. |
| `canvas sync` | Refresh the cached datasets. `--full` adds files, modules, and the calendar. |
| `canvas watch` | Stream local events. `--jsonl` for one document per line, `--since CURSOR`, `--once`. |
| `canvas notify` | Post desktop alerts for observed events. `--since CURSOR`, `--stdout`. |
| `canvas cache stats` | Row counts and size per cached dataset. |
| `canvas cache clear` | Drop the cache database. State and receipts survive. |
| `canvas cache path` | Print the cache database path. |
| `canvas config path` | Print the config file path. |
| `canvas config edit` | Open the config file in `$EDITOR`. |
| `canvas config get` | Read one config key. |
| `canvas config set` | Write one config key, validated. |
| `canvas alias set` | Give a course a short name of your own. |
| `canvas alias list` | List your course aliases. |
| `canvas alias remove` | Remove a course alias. |
| `canvas doctor` | Check config, databases, credentials, and locks. `--network`. |
| `canvas schema` | Print the JSON Schema of a command's `--json` output. `--list`. |
| `canvas mcp` | Serve the Model Context Protocol over stdio for one identity. |
| `canvas completions` | Print a completion script: bash, zsh, fish, powershell, elvish. |
| `canvas version` | Print the version, build commit, and target triple. |

`<course>` accepts a numeric Canvas ID, one of your aliases, a Canvas URL, or a
case-insensitive substring of the course code or name. `<assignment>` works the
same way inside a course.

## Shell completions and man pages

The release archives carry man pages for every command and completion scripts
for all five shells. Without them, generate a script yourself:

```sh
canvas completions zsh > "${fpath[1]}/_canvas"
canvas completions bash > ~/.local/share/bash-completion/completions/canvas
canvas completions fish > ~/.config/fish/completions/canvas.fish
```

## `--json`

Every data command takes `--json` and prints exactly one JSON document on
stdout. The envelope carries `schema`, `generated_at`, `profile`, `identity`,
`freshness`, `requests`, `partial`, `warnings`, `outcome`, `exit`, and
`result`. `schema` is `canvas-cli/<command>@<n>`: adding a field keeps `n`,
removing or changing one bumps it. IDs are decimal strings, timestamps are RFC
3339 UTC, and `null` means unknown or not applicable.

The `result` payload of each command is defined in
[`docs/SPEC.md` Appendix D](docs/SPEC.md#appendix-d-json-result-payloads-v1).

stdout carries data only. Progress, logs, and confirmations go to stderr.
`--json` disables color and progress, and the raw-output commands
(`completions`, `auth token --reveal`, `config edit`, `calendar --ics -`,
`receipts export --out -`) reject it with exit 2.

## Agents

`canvas mcp` serves the Model Context Protocol on stdin and stdout. One
instance serves one identity: pick it with `--profile`. It serves **one tool,
`getclitools`**, and that tool performs nothing: it returns the complete
`canvas` command reference — every command, its operands and flags, and the
envelope each one returns — and the agent runs the commands itself. There is
no second tool, no resource, and no subscription, so nothing on this surface
reads Canvas, writes to it, reveals a credential, changes an identity, or
touches the browser. Every read and every write is a `canvas` command, and
every write prints what it is about to do and asks at the terminal.

```json
{
  "mcpServers": {
    "canvas": { "command": "canvas", "args": ["--profile", "default", "mcp"] }
  }
}
```

[`skill/canvas-cli/`](skill/canvas-cli/SKILL.md) is the shipped skill: the
identity model, six workflows, the exit-code and recovery table, and the MCP
setup for Claude Code, Codex, and Cursor. The release archives carry it.
`docs/agent-hosts.md` records which hosts were actually exercised.

## The browser companion

`canvas bridge` attaches the Canvas tab you already have open to the agent you
are already talking to. A toolbar click in the shipped `extension/` is the
gesture; the extension checks the account with one fixed same-origin
`GET /api/v1/users/self` and sends the location, the zone, and — only when you
ask for it — the selected passage. Cookies never leave Chrome, there is no
fetch proxy, and quizzes, assessments, and unrecognized embedded tools expose
nothing at all.

```sh
canvas bridge install --extension-id <ID>   # write the native host manifest
canvas bridge status                        # manifest, broker owner, attachment
canvas here --text --json                   # the context bundle
```

[`docs/companion.md`](docs/companion.md) has the install steps, the protocol,
the zones, and an explicit record of what was run here and what was not: the
broker is tested end to end against a real host process, and no flow was
exercised in a real Chrome on this machine.

`canvas schema <command>` prints the JSON Schema of any command's envelope and
`result`, and `canvas schema --list` prints the registry: three tab-separated
columns, the name, the schema id, and whether that name is a command you can
run or a document no command prints (`error@1`, `plan@1`, `receipt@1`). Every
name in the first column resolves. `getclitools` is built on the same
registry, and carries that listing verbatim, so the reference an agent reads
and the schema a person prints cannot disagree.

## Where things live

XDG layout on macOS and Linux, AppData on Windows.

| Purpose | macOS and Linux | Windows |
|---|---|---|
| Config | `~/.config/canvas-cli/config.toml` | `%APPDATA%\canvas-cli\config.toml` |
| Credential fallback file | `~/.config/canvas-cli/credentials.toml` | not offered |
| Data root | `~/.local/share/canvas-cli/` | `%LOCALAPPDATA%\canvas-cli\data\` |
| Identity directory | `<data root>/<identity-key>/` | same |
| Cache database (disposable) | `<identity dir>/cache.sqlite` | same |
| State database (durable) | `<identity dir>/state.sqlite` | same |
| Download manifests | `<identity dir>/downloads/<dest-id>.sqlite` | same |
| Receipt exports | `<identity dir>/receipts/*.json`, mode `0600` | same |
| Identity locks | `<data root>/locks/<identity-key>.lock` | same |
| Broker ownership lock | `<data root>/bridge/<identity-key>.lock` | same |
| Broker endpoint | `<data root>/bridge/<identity-key>.sock`, mode `0600` | `\\.\pipe\canvas-cli-<identity-key>` |

`canvas cache path` and `canvas config path` print the live values.
`canvas cache clear` touches only the cache database.

## Receipts

A receipt is a **local integrity record**: it proves what this machine sent and
what the server answered or showed for that attempt. It is not a server
signature and not independent proof of deadline compliance.

## Privacy

No telemetry, no third-party servers. Requests go only to the Canvas origin you
authenticated against, and the token is never written to a log or a cache row.

## License

MIT or Apache-2.0, at your option. See [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).
