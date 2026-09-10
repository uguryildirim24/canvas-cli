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

`docs/SPEC.md` is the contract. This README is the short version.

## Install

Nothing is published yet: there is no git remote, no Homebrew tap, and no
crates.io release. The commands below are the routes the project supports, and
`docs/release.md` records what the owner runs to make them live.

```sh
# Homebrew, the supported route on macOS and Linux
brew install uguryildirim24/homebrew-tap/canvas-lms-cli

# From crates.io, once published
cargo install canvas-lms-cli

# Prebuilt archive without a compiler, once released
cargo binstall canvas-lms-cli
```

Direct archive downloads on macOS are not a supported route until the binaries
are signed and notarized, so there is no `curl | sh` installer.

To build from this repository:

```sh
cargo build --release        # binary at target/release/canvas
just check                   # fmt, clippy, tests, cargo-deny
```

## First run

```sh
canvas auth login --host lasell.instructure.com
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
| `canvas inbox` | List conversations. `--scope inbox\|unread\|sent\|archived`. |
| `canvas inbox show` | Show one conversation and its messages. |
| `canvas inbox unread-count` | Show the unread conversation count. |
| `canvas calendar` | Calendar events. `--days`, `--course`, `--ics`, `--alarm`. |
| `canvas open` | Open a course, or any Canvas URL, in the browser. |
| `canvas open assignment` | Open an assignment page in the browser. |
| `canvas open file` | Open a file by id in the browser. |
| `canvas open announcement` | Open an announcement page in the browser. |
| `canvas sync` | Refresh the cached datasets. `--full` adds files, modules, and the calendar. |
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
instance serves one identity: pick it with `--profile`. The catalog is
read-first, and it holds no tool that reveals a credential, changes an
identity, runs arbitrary HTTP or shell, clears the cache, overwrites a file,
or opens a browser. A submission still needs a recorded human approval.

```json
{
  "mcpServers": {
    "canvas": { "command": "canvas", "args": ["--profile", "default", "mcp"] }
  }
}
```

[`skill/canvas-cli/`](skill/canvas-cli/SKILL.md) is the shipped skill: the
identity model, five workflows, the exit-code and recovery table, and the MCP
setup for Claude Code, Codex, and Cursor. The release archives carry it.
`docs/agent-hosts.md` records which hosts were actually exercised.

`canvas schema <command>` prints the JSON Schema of any command's envelope and
`result`, and `canvas schema --list` prints the registry: three tab-separated
columns, the name, the schema id, and whether that name is a command you can
run or a document no command prints (`error@1`, `plan@1`, `receipt@1`). Every
name in the first column resolves. The MCP tools use the same documents as
their output schemas.

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
