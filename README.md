# canvas-cli

A Rust command-line client for reading Canvas LMS coursework, downloading files,
and submitting work with local receipts.

## Why it exists

`canvas` brings student coursework into a terminal workflow. SQLite caches make
previously fetched coursework available offline. JSON output includes freshness,
partial results, and exit codes for scripts and AI coding agents. Write commands
record intent before sending it and keep a journal for uncertain outcomes.
There are no teacher, TA, or admin commands.

[docs/SPEC.md](docs/SPEC.md) describes the command and security contracts.

## Build and run from source

Requirements: Rust 1.88 or newer, Cargo, a platform C compiler and linker for
bundled SQLite, and internet access to fetch dependencies on a clean clone.
Run these commands from the repository root:

```sh
cargo build --locked --release
./target/release/canvas --help
./target/release/canvas version
./target/release/canvas schema --list
./target/release/canvas doctor
```

On Windows, use `target\release\canvas.exe`. Commands later in this README
assume the binary is on `PATH`. On macOS or Linux, set it for this shell:

```sh
export PATH="$PWD/target/release:$PATH"
```

The release build and local commands above passed on macOS during this review.
Source builds are the runnable installation route documented here. Release
packaging is configured, but Homebrew, crates.io, and prebuilt archive installs
are not verified publication routes. See [docs/release.md](docs/release.md).

For the existing checks, install `just` and `cargo-deny`, plus Rust's `rustfmt`
and `clippy` components. `cargo-nextest` is optional. `just test` uses it when
installed and otherwise runs `cargo test`. Node.js 24 or newer runs the companion
checks without npm dependencies:

```sh
rustup component add rustfmt clippy
cargo install --locked just cargo-nextest cargo-deny
just check
npm --prefix extension test
```

`cargo deny check` needs dependency metadata and the advisory database.
In the final worktree run, `just check` passed formatting and Clippy, then
stopped at socket-dependent tests with exit 100. A separate full test run
passed 904 of 923 checks. The remaining 19 failures depend on Unix socket
paths that exceed the 103-byte endpoint limit. The approved worktree and
review roots are already too long for those tests. A short checkout and
private temporary root need separate approval before a complete rerun.
See [docs/testing.md](docs/testing.md) for isolation and results.
A failed check is not a release pass.

## First run

```sh
canvas auth login --host YOUR_CANVAS_HOST
```

Replace `YOUR_CANVAS_HOST` with the actual HTTPS Canvas hostname used by your
institution. It is a placeholder, not a bundled account. The example host
`canvas.example.test` elsewhere in this repository is not a live service.

`auth login` asks for a **personal access token**. If your institution permits
one, create it in Canvas at
`Account → Settings → Approved Integrations → + New Access Token`, or let
`auth login` open `https://<your-host>/profile/settings` for you. The token is
read from a hidden prompt, from `--token-stdin`, or from `CANVAS_TOKEN`, and
the command prints which source it used.

`canvas-cli` then calls `GET /api/v1/users/self` and stores the token in the OS
credential store (macOS Keychain, Windows Credential Manager, Secret Service on
Linux) under the identity it just confirmed. Every cache row, alias, journal,
receipt, and download is bound to that identity = (canonical origin, user ID).
If the credential backend is unavailable on macOS or Linux, login requires
explicit terminal confirmation before writing a plaintext token to
`~/.config/canvas-cli/credentials.toml` with mode `0600`. This file backend is
not offered on Windows. Auth and credential-management commands can read the
saved credential. Coursework sessions in this frozen version use only
`CANVAS_TOKEN`; login alone does not enable fresh coursework reads or sync.
Set the same token in the shell before running those commands. In Bash:

```sh
read -r -s -p 'Canvas token: ' CANVAS_TOKEN
printf '\n'
export CANVAS_TOKEN
```

Do not put the token value in shell history or a repository file.

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

These command paths are implemented and checked against the CLI command tree.
This does not establish that every path works against a live institution.

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
| `canvas download` | Download course files per module; it never replaces a file it did not write or one you changed unless you pass `--force`. `--dest`, `--module`, `--file`, `--jobs`, `--dry-run`, `--force`, `--verify`. |
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
| `canvas notify` | Print grouped event summaries to stdout. `--since CURSOR`, `--stdout` (no desktop backend). |
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

Packaging is configured to include man pages and completion scripts for all
five shells. Generate a completion script from a source build:

Choose the block for your shell.

Bash:

```sh
mkdir -p "$HOME/.local/share/bash-completion/completions"
canvas completions bash > "$HOME/.local/share/bash-completion/completions/canvas"
```

Fish:

```fish
mkdir -p "$HOME/.config/fish/completions"
canvas completions fish > "$HOME/.config/fish/completions/canvas.fish"
```

Zsh (load this directory before calling `compinit` in your shell configuration):

```zsh
mkdir -p "$HOME/.local/share/zsh/site-functions"
canvas completions zsh > "$HOME/.local/share/zsh/site-functions/_canvas"
fpath=("$HOME/.local/share/zsh/site-functions" $fpath)
autoload -Uz compinit
compinit
```

## `--json`

Every data command takes `--json` and prints exactly one JSON document on
stdout. The envelope carries `schema`, `generated_at`, `profile`, `identity`,
`freshness`, `requests`, `partial`, `warnings`, `outcome`, `exit`, and
`result`. `schema` is `canvas-cli/<command>@<n>`: adding a field keeps `n`,
removing or changing one bumps it. IDs are decimal strings, timestamps are RFC
3339 UTC, and `null` means unknown or not applicable.

The `result` payload of each command is defined in
[`docs/SPEC.md` Appendix D](docs/SPEC.md#appendix-d-json-result-payloads).

stdout carries data only. Progress, logs, and confirmations go to stderr.
`--json` disables color and progress, and the raw-output commands
(`completions`, `schema`, `notify`, `bridge host`, `auth token --reveal`,
`config edit`, `calendar --ics -`, `receipts export --out -`) reject it with
exit 2. `watch` also rejects `--json`; use `--jsonl` for its event stream.

## Agents

`canvas mcp` serves the Model Context Protocol on stdin and stdout. One
instance serves one identity: pick it with `--profile`. It serves **one tool,
`getclitools`**, and that tool performs nothing: it returns the complete
`canvas` command reference: every command, its operands and flags, and the
envelope each one returns. The agent runs the commands itself. There is
no second tool, no resource, and no subscription, so nothing on this surface
reads Canvas, writes to it, reveals a credential, changes an identity, or
touches the browser. Every read and every write is a `canvas` command, and
Canvas writes print what they are about to do and normally ask at the terminal.
The `--yes` flag explicitly bypasses that prompt.

```json
{
  "mcpServers": {
    "canvas": { "command": "canvas", "args": ["--profile", "default", "mcp"] }
  }
}
```

[`skill/canvas-cli/`](skill/canvas-cli/SKILL.md) is the shipped skill: the
identity model, seven workflows, the exit-code and recovery table, and the MCP
setup for Claude Code, Codex, and Cursor. Packaging is configured to include it.
`docs/agent-hosts.md` records which hosts were actually exercised.

## The browser companion

`canvas bridge` attaches the Canvas tab you already have open to the agent you
are already talking to. A toolbar click in the shipped `extension/` is the
gesture; the extension checks the account with one fixed same-origin
`GET /api/v1/users/self` and sends the location and the zone. It sends the
selected passage only when requested. Cookies never leave Chrome, there is no
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
`CANVAS_CONFIG_DIR` overrides the config root. Auth, identity, and credential
management use `CANVAS_DATA_DIR` for their data root. Coursework sessions use
`CANVAS_DATA_ROOT` instead. To isolate all commands, set both data variables to
the same private directory outside the checkout. With neither override set,
commands use their platform defaults.
`canvas cache clear` touches only the cache database.

## Receipts

A receipt is a **local integrity record**: it proves what this machine sent and
what the server answered or showed for that attempt. It is not a server
signature and not independent proof of deadline compliance.

## Privacy

The project has no telemetry. API calls are bound to the authenticated Canvas
origin. Downloads can follow cross-origin HTTPS storage URLs, and uploads can
use storage URLs supplied by Canvas. The Canvas bearer token is withheld from
cross-origin downloads and from multipart upload requests.

Use the hidden login prompt or `--token-stdin` for secrets. `CANVAS_HOST` and
`CANVAS_TOKEN` also provide an environment-based login. Never put real tokens in
examples, a shell command argument, or a committed `.env` file. OAuth sample
responses in the research notes use explicit placeholders; OAuth login is not
implemented.

Caches, receipts, journals, downloads, and JSON exports can contain private
academic data, including grades, assignment text, and messages. Keep them outside
this checkout. `canvas cache path` and `canvas config path` show the active paths.
Downloads go to the configured destination or `--dest`; inspect that path before
running a download. The ignore rules cover common local data paths, not every
possible export filename. Never commit account recordings or personal output.

## Example and recorded results

This is a synthetic excerpt, not an authenticated run or Rolf's coursework:

```json
{
  "courses": [{ "id": "60101", "code": "SYN-600", "name": "Synthetic name" }]
}
```

See the full canned envelope in
[the course snapshot](crates/canvas-cli/src/output/snapshots/canvas__output__registry__tests__courses_envelope_snapshot.snap).
The fixture set at `crates/canvas-api/tests/fixtures/bench-5/` declares itself
synthetic. The uncertain model payloads, public result examples, and former
institution-labeled grade seeds have been replaced with invented data. Their
construction and dependent snapshots are documented in
[fixture provenance](crates/canvas-api/tests/fixtures/README.md).

[docs/bench.md](docs/bench.md) preserves historical local mock-server measurements.
They are not live Canvas latency measurements and were not rerun here.
[docs/testing.md](docs/testing.md) records checks on the scope-trimmed tree.
Earlier checks of the refactored session do not apply to this version.
The source build and local commands passed. The 52 companion checks passed.
The coverage table, reply guide, and synthetic cache snapshot checks passed.
Runtime code matches HEAD. This is not a fully passing release check.

## Project layout

| Path | Purpose |
|---|---|
| `crates/canvas-api/` | HTTP transport, pagination, models, and API fixtures. |
| `crates/canvas-core/` | Identity, SQLite state, synchronization, transfers, journals, and receipts. |
| `crates/canvas-cli/` | The `canvas` binary, commands, JSON schemas, and output snapshots. |
| `xtask/` | Fixture recording and sanitization, mock benchmarks, and release assets. |
| `extension/` | Chrome native-messaging companion and Node.js checks. |
| `skill/canvas-cli/` | Agent command guidance and workflows. |
| `docs/` | Contracts, testing notes, local measurements, and release instructions. |

## Limits and known gaps

- Login uses personal access tokens only. OAuth is not implemented. Institutions
  may restrict student tokens. Review institutional and Canvas API policy before
  distributing or using this client.
- Group submissions and external-tool assignments are browser handoffs. New
  Quizzes cannot be taken through the CLI. Classic Quiz taking refuses LockDown,
  IP-filtered, one-question-at-a-time, and no-going-back quizzes.
- Grades are values reported by Canvas, not local estimates.
- A local receipt records an attempt and its evidence. It is not a server
  signature or independent proof of meeting a deadline.
- The MCP server returns a command reference, not Canvas data. An agent needs
  shell access to use the commands. Current third-party host behavior has not
  been rechecked after the one-tool change.
- The Chrome companion remains unverified in a real browser in this review.
  An earlier report describes a broker disconnect after navigation. It is not
  established that this is fixed. See [docs/companion.md](docs/companion.md).
- Terminal approval authorizes the exact action. It does not establish that AI
  assistance is permitted by a course's rules.
- Authenticated operations and cross-platform builds were not verified during
  this scope review. Rust 1.88 passed the workspace check on macOS.

## How this was built

AI coding agents did much of the design, implementation, and review under Rolf's
direction. Rolf set the publication requirements: keep private data out of the
repository, match claims to recorded checks, and leave commits and pushes to
review. Agents checked source builds, local commands, and mock-based tests.
The repository also records local benchmarks. Authenticated coursework and real
Chrome interaction were not checked in this publication review. The repository
does not establish which features Rolf personally verified.

## License

MIT or Apache-2.0, at your option. See [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).
