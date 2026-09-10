# canvas-cli — Specification

Status: v0.9, 2026-09-10. Owner: Rolf. Supersedes v0.8. Research inputs: `docs/research/r1-canvas-api.md`, `r2-prior-art.md`, `r3-rust-stack.md`, and `docs/agent-ux/REPORT.md` for the post-v1 rounds. §§1–19 and Appendices A–E are the v1 contract. §§20–25 record what the post-v1 packages **built**, as `main` has them; Appendix C names those packages and their review files. There are no placeholders left: every section of this document describes code on `main`. §19 holds the questions still open for the owner, and Appendix E maps every round-1 to round-7 finding to its resolution.

## 0. Summary

`canvas-cli` is a Rust command-line client for Canvas LMS (Instructure), built for **students only**. One binary, `canvas`, gives a student their deadlines, Canvas-reported grades, course files, announcements, and a way to submit work with a durable journal and a local receipt, without opening the Canvas web app. It is fast because it keeps a local SQLite cache. It is safe because the API token lives in the OS credential store and is sent only to its own origin. It is scriptable because every data command has a defined `--json` schema.

The first user is the owner, a student at Lasell University (`courses.example.test`). The owner confirmed on 2026-09-09 that a personal access token works on that instance. The design works for any Canvas instance.

## 1. Goals and non-goals

### Goals (v1)

1. **Daily driver.** `canvas todo` shows what is due and what is missing, from cache in well under a second and with a small, measured number of API calls when fresh.
2. **Submit from the terminal.** Upload files, text, or a URL to an assignment, with a transactional journal of every step and a receipt bound to the exact attempt the server created.
3. **Module-aware download.** Fetch the files a course exposes through its Files listing and its module file items, organized per module, never overwriting a file the CLI did not itself write, with honest reporting of what was locked or unavailable.
4. **Grades as Canvas reports them.** Current and final scores per course, per grading period, assignment-group weights and rules, and per-assignment scores. Local grade estimation is v2 (§12.4).
5. **Scriptable.** A versioned `--json` schema per command (Appendix D), with freshness, coverage, and partial-result metadata.
6. **Secure by default.** Token in the OS credential store, bound to one origin and one user. No telemetry. No third-party servers. No capability-bearing URLs on disk.
7. **Cheap on the API.** TTL cache, `per_page=100`, bounded concurrency, adaptive throttling.

### Non-goals

- **No teacher, TA, or admin tools.** Only endpoints a student role can call.
- **No quiz taking.** Quizzes appear in `todo` and `assignments` with a browser handoff.
- **No LTI launches.** External-tool assignments get an "opens in browser" action only.
- **No group submissions in v1.** Assignments with a group category are refused before upload with a browser hint.
- **No OAuth2 developer-key flow in v1.** Personal access tokens only. Canvas requires OAuth for applications used by many users, so distribution to other students is conditional on adding OAuth later.
- **No browser automation and no cookie import.** If an institution blocks student tokens, v1 cannot log in there.
- **No local grade calculator, no GraphQL, no conditional requests (ETags) in v1.**
- **No GUI.** An optional TUI dashboard is v2.
- **Not a general Canvas SDK.**

## 2. Users and jobs

| Who | Job | Command |
|---|---|---|
| Student, morning check | What is due this week? What did I miss? | `canvas todo` |
| Student, before a deadline | Submit `hw3.pdf` and keep proof of what was sent | `canvas submit`, `canvas receipts` |
| Student, start of term | Get the slides for CHEM 301 into a folder | `canvas download chem` |
| Student, mid-term | What grade does Canvas show me, and how is it weighted? | `canvas grades` |
| Student, offline on the train | Read the assignment prompt | `canvas assignment ... --offline` |
| Student, scripting | Pipe deadlines into a calendar or an agent | `canvas todo --json`, `canvas calendar --ics` |

## 3. Design principles

1. **Cache first, network second.** Reads serve from SQLite when the data is inside its TTL and its recorded coverage contains the request. `--fresh` forces a fetch. `--offline` forbids one.
2. **One identity per action.** Every network call, cache row, alias, journal, receipt, and download manifest is bound to an identity = (canonical origin, user ID).
3. **Short commands, typed targets.** `canvas assignments chem` resolves "chem" from a complete cached course list. Ambiguous kinds take typed targets.
4. **Two output modes, one source of truth.** Human tables and `--json` render the same normalized structs. Every invocation emits exactly one JSON document in `--json` mode.
5. **stdout is data, stderr is everything else.**
6. **Respect the platform.** A lock or a denial is reported, not worked around. No ID probing.
7. **Write paths journal first, in a transaction.** `submit` commits intent to the state database before the first upload and commits every server answer as it arrives.
8. **Never clobber.** The CLI replaces only files it wrote and can prove it wrote.
9. **Honest claims.** Coverage, freshness, unknown values, and partial failures are always visible.

## 4. Naming

| Thing | Name | Note |
|---|---|---|
| Project and GitHub repo | `canvas-cli` | `github.com/uguryildirim24/canvas-cli` |
| Binary | `canvas` | Installers may add a `canvas-cli` symlink. |
| crates.io name | `canvas-lms-cli` | `canvas-cli` is taken. Free on 2026-09-09. Publishing is optional. |
| Credential-store service | `canvas-cli` | Account = identity key, §8. |
| Config and data dirs | `canvas-cli` | §9. |
| User-Agent | `canvas-cli/<version> (+https://github.com/uguryildirim24/canvas-cli)` | |

## 5. Command surface

### Global flags

| Flag | Meaning |
|---|---|
| `--json` | Machine-readable output (§7, Appendix D). |
| `--color auto\|always\|never` | Honors `NO_COLOR`, `CLICOLOR_FORCE`, TTY detection. |
| `--profile NAME` | Profile selection, §8 matrix. |
| `--fresh` | Ignore cache TTLs. Conflicts with `--offline` (exit 2). |
| `--offline` | Never touch the network. See the §8 offline matrix. |
| `-q, --quiet` | No progress, no info logs. |
| `-v, --verbose` | Debug logs on stderr, redacted per §11. |

### Commands (v1)

```
canvas auth login [--host HOST] [--token-stdin] [--replace]
canvas auth status
canvas auth logout
canvas auth token --reveal
canvas identity list
canvas identity remove <identity-key> [--yes]

canvas courses [--all] [--term TEXT] [--favorites]
canvas course <course>

canvas todo [--days N] [--all] [--missing] [--course <course>]

canvas assignments <course> [--bucket open|upcoming|overdue|past|undated|unsubmitted|ungraded|future|all] [--search TEXT]
canvas assignment <course> <assignment>
canvas assignment <url>

canvas submit <course> <assignment> --file PATH [--file PATH ...] [--comment TEXT] [--yes]
canvas submit <course> <assignment> --text PATH|- [--comment TEXT] [--yes]
canvas submit <course> <assignment> --html PATH [--comment TEXT] [--yes]
canvas submit <course> <assignment> --url URL [--comment TEXT] [--yes]
canvas submit <url> ...
canvas submission <course> <assignment> [--history]
canvas submission verify <receipt-id>
canvas submission reconcile <journal-id> [--assume-not-submitted]

canvas receipts list [--course <course>] [--state STATE]
canvas receipts show <receipt-id|journal-id>
canvas receipts export <receipt-id> [--out PATH]
canvas receipts acknowledge <journal-id>

canvas grades [<course>] [--period current|all|ID]

canvas files <course> [--tree] [--search TEXT]
canvas download <course>|--all-courses [--dest DIR] [--module TEXT] [--file ID ...] [--jobs N] [--dry-run] [--force] [--verify]

canvas modules <course> [--items]

canvas announcements [<course>] [--since DURATION] [--unread]
canvas announcement <course> <id>
canvas announcement <url>

canvas calendar [--days N] [--course <course>] [--ics PATH|-] [--alarm DURATION]

canvas open <course>
canvas open assignment <course> <assignment>
canvas open file <id>
canvas open announcement <course> <id>
canvas open <url>

canvas sync [--full]
canvas cache stats|clear|path

canvas config path|edit|get KEY|set KEY VALUE
canvas alias set NAME <course> | canvas alias list | canvas alias remove NAME

canvas doctor [--network]
canvas completions bash|zsh|fish|powershell|elvish
canvas version
```

### Commands added after v1

```
canvas pages <course> [--unpublished]
canvas page <course> <url-slug|id|URL>
canvas syllabus <course>

canvas discussions <course> [--unread]
canvas discussion <course> <id|URL> [--replies] [--page N]

canvas inbox [--scope inbox|unread|sent|archived]
canvas inbox show <id>
canvas inbox unread-count

canvas watch [--jsonl] [--since CURSOR] [--once]
canvas notify [--since CURSOR] [--stdout]

canvas schema <command> | canvas schema --list
canvas mcp

canvas bridge install [--extension-id ID] [--browser chrome|chromium|edge]
canvas bridge host [CALLER_ORIGIN] [--parent-window HANDLE]
canvas bridge status
canvas bridge detach [--attachment ID]
canvas here [--attachment ID] [--text]
canvas note --text T [--source-ref REF]… [--attachment ID] [--generation N]
canvas open <target> --follow [--attachment ID]

canvas discussion reply <course> <topic|URL> [--to ENTRY_ID] (--text T | --text-file P | --text -) [--attach P]… [--yes]
canvas inbox send --to USER_ID[,…] [--subject S] (--text T | --text-file P | --text -) [--attach P]… [--yes]
canvas inbox reply <conversation_id> (--text T | --text-file P | --text -) [--attach P]… [--yes]
canvas operation status <journal_id>
canvas operation reconcile <journal_id> [--assume-not-posted]
```

`pages`, `page`, `syllabus`, `discussions`, `discussion`, and `inbox *` are §23. `watch` and `notify` are §22. `schema` and `mcp` are §21. `bridge *`, `here`, `note`, and `open --follow` are §24. `discussion reply`, `inbox send`, `inbox reply`, and `operation *` are §25.

Command names still reserved for a later round (no contract in this document): `grades estimate|what-if|target`, `dashboard`, and `submit --resume`.

### Behaviour notes

- **`auth login`** normalizes `--host` (or the prompt answer) to a canonical origin (§8), offers to open `https://<origin>/profile/settings`, reads the token from a hidden prompt, `--token-stdin`, or `CANVAS_TOKEN` (in that order of explicitness; the source is printed), calls `GET /api/v1/users/self`, and stores the token under the resulting identity in exactly one store (§8). It then creates or updates the profile named by `--profile` (a new name is allowed here and only here) or `default`. If the profile exists and points at a different identity, it stops with exit 3 unless `--replace`, which rebinds the label; the old identity's data stays until `identity remove`. It prints the user name and the credential backend used.
- **`identity list|remove`** show identities with their profiles and on-disk sizes; `remove` runs the removal protocol in §10 after confirmation: credentials first, then the identity directory, then profiles that point at it (and `default_profile` if it was one of them).
- **`courses`** default = `enrollment_type=student&enrollment_state=active`, sorted by course code. `--all` adds `completed` and `invited_or_pending` (separate requests, same dataset scope `all`). `--term TEXT` filters locally on `term.name`. Courses Canvas marks access-restricted carry `restricted = true`.
- **`todo`** §12.1.
- **`assignments`** fetches the complete list with `include[]=submission` and applies buckets locally (§12.1). Default bucket `open` as defined in §12.1. `all` = no filter. `--search` = local substring on name.
- **`assignment`** prints the prompt as Markdown, effective dates, submission types, allowed extensions, attempts, `can_submit`, your submission, rubric criteria, and the URL. Rubric assessment feedback needs one extra request and is fetched only when a graded submission exists. `external_tool` prints the tool name and the `open` command.
- **`submit`**, **`submission`**, **`receipts`** §12.2.
- **`grades`** §12.4.
- **`files`**, **`download`**, **`modules`** §12.3.
- **`announcements`**, **`announcement`** §12.6. Bare announcement IDs are not accepted; use `<course> <id>` or a URL.
- **`calendar`** §12.5.
- **`open`** takes a typed target or a URL. It needs an identity for the origin and never fetches: numeric IDs and URLs build the target directly; a name is resolved only over a complete cached dataset, otherwise the command exits 6 with `use a numeric ID or a URL`. A URL whose origin differs from the active identity's origin is refused (exit 6).
- **`sync`** refreshes courses, assignments, submissions, missing, the default planner window, enrollment grades, and announcements for active courses. `--full` adds folders, files, modules, module items, and calendar events.
- **`pages`**, **`page`**, **`syllabus`**, **`discussions`**, **`discussion`**, **`inbox`**, **`inbox show`**, **`inbox unread-count`** §23. Every one is a `GET` and marks nothing read.
- **`watch`**, **`notify`** §22. `watch --json` is exit 2 and names `--jsonl`; `notify` is a raw-output command.
- **`schema`**, **`mcp`** §21.
- **`bridge install|host|status|detach`**, **`here`**, **`note`**, **`open --follow`** §24. `bridge host` is raw output: Chrome starts it and it speaks native messaging on stdin and stdout. `here` is the one companion command that reaches Canvas, because its API half calls the `course`, `assignment`, and `announcement` cores.
- **`discussion reply`**, **`inbox send`**, **`inbox reply`** §25. Each freezes a plan, prints it, asks for confirmation at the terminal, and needs a recorded approval before anything is sent. `--yes` is recorded as the `yes-flag` channel, never as an interactive answer.
- **`operation status|reconcile`** §25. `status` reads the thread back and changes no journal state; `reconcile` may move a journal out of `outcome_unknown`, and only on evidence.
- **`doctor`** selects an identity like any class-B command when one is selectable and otherwise runs only the identity-free checks. Local checks: config parse, profile and identity, active credential source and stray or pending-cleanup entries, DB integrity and schema versions, credential backend status, identity lock, journals with an absent owner (recovered per §12.2). Only with `--network`, network checks (`GET /users/self` id equals the profile's user id, `X-Rate-Limit-Remaining`, clock skew from the `Date` header). Without `--network`, or with `--offline`, network checks are reported as `skipped`.

### Command classes

Every command belongs to exactly one class. The class decides identity selection (§8) and the offline rule (§8).

| Class | Commands |
|---|---|
| A. identity-free, local | `version`, `completions`, `config *`, `identity list`, `schema` |
| B. identity-bound, local | `alias *`, `receipts list\|show\|export\|acknowledge`, `cache *`, `identity remove` (operand selects the identity), `open` (browser launch only, and `--follow`, which reaches the broker socket only), `auth status`, `auth logout`, `auth token`, `doctor` (without `--network`; falls back to the identity-free subset when no identity can be selected), `notify`, `bridge install\|host\|status\|detach`, `note` |
| C. identity-bound, cache-backed read | `courses`, `course`, `todo`, `assignments`, `assignment`, `submission` (without `verify`/`reconcile`), `grades`, `files`, `modules`, `announcements`, `announcement`, `calendar`, `pages`, `page`, `syllabus`, `discussions`, `discussion`, `inbox`, `inbox show`, `inbox unread-count`, `here` |
| D. network-required | `auth login`, `submit`, `submission verify`, `submission reconcile`, `sync`, `download`, `doctor --network`, `watch`, `discussion reply`, `inbox send`, `inbox reply`, `operation status`, `operation reconcile` |

`canvas here` is class C because its API half calls the `course`, `assignment`, and `announcement` cores; its browser half reaches the broker socket and nothing else. `operation status --offline` returns the stored journal rather than the class-D exit 2 `operation reconcile` gives; §19 item 37 records that exception.

`canvas mcp` has no class of its own. It binds one identity locally at startup and refuses to start without one (exit 3); the process itself opens no network connection, and each tool takes the class of the command behind it.

`auth login` is the one class-D command that runs without an existing identity; it creates one. Class-B commands never open a network connection. The class-B commands that read or change the credential store locally are `auth status`, `auth token`, `auth logout`, `identity remove`, and `doctor`; every other command touches the store only when a network call needs the token (§13).

## 6. Identifiers and resolution

`<course>` accepts, in order:

1. A numeric Canvas course ID. No fetch.
2. An alias of the active identity.
3. A Canvas URL containing `/courses/<id>`; its origin must equal the identity origin, else exit 6.
4. A case-insensitive substring of `course_code`, then `name`. Matching runs first over the members of the complete cached `courses:active` dataset; if that yields zero matches, over the union of the `courses:active` and `courses:all` memberships (never over unreferenced entity rows). Exactly one match resolves; zero or many prints candidates and exits 6.

`<assignment>` accepts a numeric ID, a Canvas URL containing `/courses/<id>/assignments/<id>` (origin check; when a course argument is also given the two course IDs must agree, else exit 6), or a substring of the assignment name over the complete cached `assignments:course:<id>` dataset.

"Complete" means the dataset's `fetch_log` row records every page stored (§10). If a needed dataset is incomplete or absent: class-C and class-D commands fetch it online and count the request, or exit 7 offline; class-B commands (`alias set`, `receipts --course`, `open`) never fetch and exit 6 with `use a numeric ID or a URL`. Numeric IDs and validated URLs never trigger a list fetch.

`<receipt-id>` and `<journal-id>` are the identifiers printed by `submit` and listed by `receipts list`. `<identity-key>` is the key defined in §8.

## 7. Output contract

### Human output

- `comfy-table`, borderless preset, two-space padding, wrap to terminal width, `…` truncation.
- Dates in the identity time zone (`users/self.time_zone`, else system zone): `Tue Sep 15, 11:59 PM` plus `(in 2d 4h)` or `(overdue 3h)`. All-day dates show the civil date only.
- Status labels are derived from the separate fields in §12.1: `missing`, `late · graded 45/50`, `submitted`, `closed`, `locked`, `external`, `pending` (a journal is unresolved for this assignment), `unknown`.
- Colors: overdue and missing red, due in under 24 h yellow, submitted green, locked and closed dim. Non-TTY output is plain.

### `--json` output

Exactly one JSON document per invocation on stdout. Envelope:

```json
{
  "schema": "canvas-cli/todo@1",
  "generated_at": "2026-09-09T17:05:12Z",
  "profile": "lasell",
  "identity": { "origin": "https://courses.example.test", "user_id": "12345", "key": "courses.example.test-12345-3f9a1c2e" },
  "freshness": [ { "dataset": "courses", "scope": "active", "source": "cache", "fetched_at": "…", "complete": true, "count": 5, "stale": false } ],
  "requests": { "api": 0, "storage": 0, "cost": null },
  "partial": [ { "scope": "announcements:course:45679", "http_status": 403, "message": "…" } ],
  "warnings": [ "…" ],
  "outcome": "ok",
  "exit": 0,
  "result": { }
}
```

Rules:

- `schema` is `canvas-cli/<command>@<n>`; Appendix D defines `result` for each command. Adding a field keeps `n`; removing or changing a field bumps it.
- IDs are decimal strings in JSON. Internally `i64`.
- Timestamps are RFC 3339 UTC. Fields marked `ts+local` in Appendix D carry a `<name>_local` sibling in the identity zone. Civil dates are `YYYY-MM-DD`.
- Absent and `null`: a field defined in Appendix D is always present; `null` means unknown or not applicable, and Appendix D says which when it matters. Arrays are never `null`; where "none" and "unknown" differ, an empty array is paired with a sibling boolean (for example `rubric_assessed`).
- `profile` and `identity` are `null` for class-A commands and for errors raised before identity selection.
- `outcome` is `ok`, `partial`, `recovery`, `mismatch`, `refused`, or `error`; `exit` is the process exit code (§14). On `error`, `result` is `{ "code": "auth", "message": "…", "http_status": 401, "server_errors": [], "details": {} }` and the schema is `canvas-cli/error@1`. When an error aborts a command after a durable side effect, `details` carries `journal_id`, the journal `state`, and any known `posted` identity, or the per-file results already produced.
- `requests` is `{ "api": n, "storage": n, "cost": x }`, where `cost` is the sum of `X-Request-Cost` values seen (`null` when none).
- Raw-output commands reject `--json` with exit 2: `completions`, `auth token --reveal`, `config edit`, `calendar --ics -`, `receipts export --out -`, `schema`, `notify`, `bridge host` (which speaks Chrome native messaging on stdin and stdout, §24). Clap usage errors and `--help` keep clap's text output.
- `canvas watch` also rejects `--json` with exit 2 and names `--jsonl`. Its stream is a separate contract (§22): one self-describing `event@1` document per line, closed by one `watch@1` envelope. This rule for `--json` is unchanged.
- `--json` disables color and progress.

### Streams and confirmations

stdout carries data only. Progress, logs, and confirmations go to stderr. Confirmations read from the controlling terminal (`/dev/tty`, `CONIN$`). With no controlling terminal and no `--yes`, a command that needs confirmation exits 2 before any network write. `--text -` consumes stdin for content only.

## 8. Authentication and identity

### Canonical origin and identity key

Origin = `https://` + lowercase IDNA-ASCII host + `:port` only when the port is not 443. Accepted hosts: DNS names, IPv4 literals, and IPv6 literals in brackets (serialized in RFC 5952 canonical form). Identity = (origin, user_id).

The **identity key** names directories, credential accounts, and JSON `identity.key`. It is filesystem-safe on every supported platform and collision-resistant:

```
<host-slug>[_<port>]-<user_id>-<digest8>
host-slug = host with every character outside [a-z0-9.-] replaced by "_"
digest8   = first 8 hex characters of SHA-256(origin + "\n" + user_id)
```

Examples: `courses.example.test-12345-3f9a1c2e`; origin `https://[::1]:8443`, user 7 → host slug `___1_`, key `___1__8443-7-<digest8>`. Each identity directory holds `identity.json` = `{ "origin", "user_id", "key", "created_at", "generation": "<uuid>" }`. The CLI verifies origin, user id, and key on every open and refuses a mismatch (exit 13). Display uses the full origin and user id, never the key alone.

### Selection matrix

Selection runs per command class (§5).

| Inputs | Class A | Classes B, C | Class D |
|---|---|---|---|
| `--profile X` or `CANVAS_PROFILE=X` | ignored | X must exist (exit 3); its identity is used | same; `auth login` may name a new X |
| no profile input, `CANVAS_HOST` + `CANVAS_TOKEN` | ignored | ephemeral profile `env`: identity from the env binding file (below) when present; otherwise class B exits 3 (`run any online command or auth login to bind this token`), class C exits 3 with `--offline` or validates online | validated online; `auth login` treats them as host and token inputs |
| exactly one of `CANVAS_HOST`, `CANVAS_TOKEN` | ignored | `CANVAS_TOKEN` alone applies to the selected or default profile; `CANVAS_HOST` alone is exit 2 | same |
| none; `default_profile` set | ignored | that profile | that profile |
| nothing | runs | exit 3 with the login hint | `auth login` runs; others exit 3 |

`CANVAS_HOST` is ignored with a warning whenever a profile is selected explicitly. Aliases belong to the identity (state DB), not to the profile. `identity remove <key>` ignores the matrix: the operand names the identity, and the command takes the exclusive identity lock directly without first holding a shared one.

**Env binding file.** After a successful online validation of an env pair, the CLI records the binding in `<data root>/env-bindings.toml`: `[bindings."<sha256(origin + "\n" + token)>"] key = "<identity key>"`, mode `0600`. Writes take an exclusive lock on `<data root>/env-bindings.lock`, read the file, modify in memory, write a `create_new` temp file with mode `0600`, `fsync`, rename over, `fsync` the directory. A consumer of a binding then opens the identity normally and verifies `identity.json`; a missing or mismatching identity means the binding is stale and is removed under the same lock. Selection through this file never opens a credential store.

### Token resolution and validation

The state DB `credential` row (one per identity, never deleted except by `identity remove`) records `active_source` (`keyring`, `file`, or `none`), `token_sha256`, `validated_at`, and two cleanup flags `cleanup_keyring`, `cleanup_file`. **Activation protocol** for `auth login`: (1) write the token to the chosen store; (2) one state transaction sets `active_source`, `token_sha256`, `validated_at`, **clears the cleanup flag of the chosen store** (a flag left by an earlier failed logout or store switch must never delete the credential just installed), and sets the cleanup flag of the other store; (3) best-effort: attempt every flagged cleanup, which by construction never targets the active store; each success clears its flag; a failure (including an unavailable backend, which is exactly the case in which the fallback file was chosen) leaves the flag set and prints a warning. Pending cleanup **never blocks** a login or a token rotation. A crash between (1) and (2) leaves a store entry whose hash differs from the recorded one; a crash between (2) and (3) leaves flags set. `doctor` and `auth status` report both as `stray` or `pending cleanup`. **Logout protocol**: (1) one state transaction sets `active_source = none`, clears `token_sha256`, and sets the cleanup flag of the former active store and of any store that `doctor` would report as stray; (2) attempt every flagged deletion; each success clears its flag; any failure exits 13 with the flags kept (the token is already unusable by this CLI because resolution rejects `none`). Resolution for use: `CANVAS_TOKEN` → the entry in `active_source` whose hash equals `token_sha256` → exit 3 (`stored token does not match the recorded one; run auth login`), `active_source = none` or no row → exit 3 with the token-creation steps. Entries outside the active source are never used. Concurrent `auth login`/`auth logout` for the same identity serialize on `<data root>/locks/<identity-key>.cred.lock` (exclusive).

Validation (`GET /api/v1/users/self`) happens the first time a token hash is seen for the identity and on every `auth login`. Outcomes:

| Response | Result |
|---|---|
| `200`, `id` equals the identity's `user_id` | validated; hash and time recorded |
| `200`, different `id` | exit 3 `identity mismatch`; nothing is written or joined with the identity's data |
| `401` | exit 3 `token rejected` (invalid or revoked); no fallback to another store |
| network failure | exit 4 |
| credential store `Denied`/`Locked`/`Backend` | exit 13 |

Offline reads (classes B and C) never need the token.

### Credential store

- `keyring = { version = "=4.2.0", default-features = false, features = ["v1"] }`, which selects `apple-native-keyring-store` (feature `keychain`), `windows-native-keyring-store`, and `zbus-secret-service-keyring-store` (feature `crypto-rust`) through `keyring-core 1`. MSRV 1.88. M0-c proves the lock file resolves and builds on the MSRV and on every release target, and exercises set/get/delete on macOS at least.
- Entry: service `canvas-cli`, account = identity key.
- Error mapping (application enum; keyring's own errors are never `Debug`-formatted because some variants carry secret bytes): keyring `NoEntry` → `NotFound`; `NoStorageAccess` and platform access/permission errors → `Denied`; `NoDefaultStore` → `Backend` only when `Entry::store_status` reports that no store is available, otherwise `Denied`; anything else → `Backend` with the sanitized message. Only `Backend` at `auth login` time offers the fallback file, after an explicit prompt. `Locked` is reported when the platform reports a locked keychain. `Denied`, `Locked`, and `Backend` outside login exit 13 and never downgrade.
- Fallback file `<config>/credentials.toml`: one `[identities."<key>"]` table per identity. Not offered on Windows. Read: open with `O_NOFOLLOW`; `fstat` must show a regular file owned by the current user with mode `0600`, else the read fails with `Unsafe` (exit 13, message names the file). Write (add, replace, delete one entry): take an exclusive `flock` on `<config>/credentials.lock`; open the file as above (`ENOENT` → start from an empty map; any other failure → exit 13, no write); modify in memory; write to a `create_new` temp file with mode `0600` in the same directory; `fsync`; rename over; `fsync` the directory; release the lock.
- `auth status` prints the active source, the identity, and stray or pending-cleanup entries. `auth logout` follows the logout protocol above and warns if `CANVAS_TOKEN` is still set. Logout is local.
- The token type `Secret` lives in `canvas-api`; `Debug` and `Display` print `[redacted]`.

### Offline matrix

| Class | With `--offline` |
|---|---|
| A | runs |
| B | runs; `auth logout` and `identity remove` still touch the local credential store |
| C | serve from cache; success is decided by coverage metadata (a complete dataset with `count = 0` is success); absent coverage exits 7; stale coverage is served with `stale: true` |
| D | exit 2 before authentication or I/O |

## 9. Config and paths

XDG layout on every Unix, including macOS. Windows uses AppData. Implemented with `etcetera` (`choose_base_strategy`).

| Purpose | macOS and Linux | Windows |
|---|---|---|
| Config | `~/.config/canvas-cli/config.toml` | `%APPDATA%\canvas-cli\config.toml` |
| Credentials fallback + lock | `~/.config/canvas-cli/credentials.toml`, `credentials.lock` | not offered |
| Data root | `~/.local/share/canvas-cli/` | `%LOCALAPPDATA%\canvas-cli\data\` |
| Env binding file | `<data root>/env-bindings.toml` | same |
| Identity locks (never deleted) | `<data root>/locks/<identity-key>.lock`, `<identity-key>.cred.lock` | same |
| Coordinator locks (never deleted, §22) | `<identity dir>/locks/api-slot-<n>.lock`, `refresh-<dataset>-<scope>.lock`, `interest-assignment-<id>.lock` | same |
| Broker endpoint (§24) | `<data root>/bridge/<identity-key>.sock` in `<data root>/bridge/` (dir `0700`, socket `0600`) | named pipe `\\.\pipe\canvas-cli-<identity-key>`, DACL `D:P(A;;GA;;;{owner SID})(A;;GA;;;SY)` |
| Broker ownership lock (§24) | `<data root>/bridge/<identity-key>.lock`, held exclusively for the host's lifetime; removed only by `identity remove` | same |
| Env binding lock | `<data root>/env-bindings.lock` | same |
| Identity dir | `<data root>/<identity-key>/` | same |
| Cache DB (disposable) | `<identity dir>/cache.sqlite` | same |
| State DB (durable) | `<identity dir>/state.sqlite` | same |
| Journal owner and admission locks | `<identity dir>/journals/<journal-id>.lock`; admission by target: `assignment-<id>.lock` (§12.2), `topic-<tid>.lock`, `conversation-<id>.lock`, `conversation-new-<plan-id>.lock` (§25) | same |
| Download manifests and locks (one per destination) | `<identity dir>/downloads/<dest-id>.sqlite`, `<dest-id>.lock` | same |
| Receipt exports | `<identity dir>/receipts/*.json` (mode `0600`) | same |
| Verify scratch | `<identity dir>/tmp/` | same |

`config.toml` example:

```toml
default_profile = "lasell"

[profiles.lasell]
origin    = "https://courses.example.test"
user_id   = 12345
key       = "courses.example.test-12345-3f9a1c2e"
name      = "Rolf"
time_zone = "America/New_York"

[download]
dest = "~/School/2026-fall"
jobs = 4

[cache]
ttl_courses       = "6h"
ttl_assignments   = "10m"
ttl_planner       = "10m"
ttl_missing       = "10m"
ttl_grades        = "10m"
ttl_modules       = "1h"
ttl_files         = "1h"
ttl_announcements = "15m"
ttl_calendar      = "1h"
ttl_pages         = "1h"
ttl_discussions   = "15m"
ttl_inbox         = "5m"

[bridge]
extension_id       = "abcdefghijklmnopabcdefghijklmnop"
pause_hidden_after = "10m"

[network]
api_concurrency     = 4
storage_concurrency = 4

[output]
color = "auto"
```

Precedence: defaults → `config.toml` → `CANVAS_*` env → flags, via `figment`. `config set` validates keys.

`cache.ttl_pages`, `cache.ttl_discussions`, and `cache.ttl_inbox` were added by M8-a (§23). M7-a added the two `bridge.*` keys (§24):

| Key | Default | What it does |
|---|---|---|
| `bridge.extension_id` | none | the only extension the broker will serve. `bridge install --extension-id ID` writes it |
| `bridge.pause_hidden_after` | `10m` | how long a hidden tab keeps sharing. `<n>h`, `<n>m`, or `<n>s`; zero is rejected |

The `governor`, `interest`, `observations`, `baselines`, `events`, `consumer_cursor`, `plans`, `approval_handles`, and `operation_journal` tables all live in the identity's `state.sqlite`, so `cache clear` cannot reach them (§20, §22, §25).

## 10. Cache, state, and sync

### Databases and locking

- Three kinds of database: **cache.sqlite** (disposable), **state.sqlite** (durable), and one **download manifest** per destination under `<identity dir>/downloads/` (durable, §12.3). All of them: `rusqlite` bundled, WAL, `busy_timeout = 5000`, `PRAGMA user_version`, migrations inside a single transaction, refusal to open a newer schema (exit 13), `BEGIN IMMEDIATE` for every write transaction, and access through the single SQLite thread below. Every database lives inside the identity directory, so SQLite's own path-based opens (database, `-wal`, `-shm`, journal) never touch a user-chosen destination. Network waits never happen inside a transaction. File locks use `fs4` (`flock` on Unix, `LockFileEx` on Windows).
- **Identity lock.** Every process that opens an identity takes a **shared** lock on `<data root>/locks/<identity-key>.lock` for its lifetime, then verifies `identity.json` (origin, user id, key, generation) and keeps the generation in memory. The lock file is never deleted.
- **Identity removal** (`identity remove`): take the identity lock **exclusively** (5 s timeout, else exit 13); re-read `identity.json`; delete credential entries (any failure → exit 13, nothing else removed); delete the identity directory; remove profiles that reference the key and clear `default_profile` if it was one of them; release. A process that was waiting on the lock re-reads `identity.json` after acquiring it; a missing directory or a different generation means the identity is gone or was recreated, and the process exits 13 with `identity changed`.
- `cache clear` runs `DELETE` on every cache table in one transaction followed by `VACUUM`; it never unlinks an open database. Mutation epochs (below) live in `state.sqlite`, so a clear cannot erase them.
- All SQLite access runs on one dedicated thread per process fed by a bounded channel. The coordinator's governor connection is the one exception (§22, §19 item 22).

### Migration list

Each database keeps its own `PRAGMA user_version` and its own ordered batch list. A batch is applied inside one transaction, and a database already at its current version runs none.

| Migration | Database | Package | Adds |
|---|---|---|---|
| `0001_initial` | both | M1-a | the v1 cache and state schema |
| `0002_reads` | cache | M8-a | `pages`, `discussion_topics`, `discussion_entries`, `conversations`, `conversation_unread` (§23) |
| `0002_plans` | state | M6-a | `plans`, `approval_handles`, the journal plan link and its partial unique index (§20) |
| `0003_events` | state | M6-c | `governor`, `interest`, `observations`, `baselines`, `events`, `consumer_cursor` (§22) |
| `0004_operations` | state | M8-b | `operation_journal` with the unique index `operation_journal_plan ON operation_journal(plan_id)` and indexes on state and course, plus the nullable column `plans.operation_json` (§25) |

`CACHE_USER_VERSION` is 2 and `STATE_USER_VERSION` is 4. A database at a newer version is refused (exit 13).

`plans.course_id` and `plans.assignment_id` stay `NOT NULL` and hold `0` for a write that names no course and no assignment; `plans.operation_json` carries the real target, and `plan@1` prints `null` for the fields that mean nothing there (§25.2).

### Entities, observations, membership, coverage

Cache tables split into **entity** tables (`courses`, `terms`, `assignment_groups`, `assignments`, `submissions`, `modules`, `module_items`, `folders`, `files`, `announcements`, `calendar_events`, `planner_items`, `enrollment_grades`, `grading_periods`), one **membership** table (`membership(dataset, scope, entity_kind, entity_id, position)`), and **`fetch_log(dataset, scope, fetched_at, complete, count, stale, error, epoch_seen, contexts, window_start, window_end)`**.

**Scoped values.** Entities whose values depend on the query carry the qualifier in their key: `enrollment_grades(enrollment_id, period)` with `period ∈ {none, <id>}`; `course_totals(course_id, mode)` with `mode ∈ {all, current}`. Assignment-group lists per period are memberships of `assignment_groups:course:<id>:period:<p>`; the group entity itself (name, weight, rules) does not vary. A read for period P joins only rows keyed with P.

**Per-field observations and freshness.** The table `field_obs(entity_kind, entity_key, field, observed_at)` records when each field of an entity was last supplied by a source. `entity_key` is the **complete normalized key** of the value row, the same key the entity table uses: the Canvas ID for plain entities, `<enrollment_id>|<period>` for `enrollment_grades`, `<course_id>|<mode>` for `course_totals`, and any future qualifier. Clock reads and value writes for one row happen in one transaction on that key, so an observation for period P can never suppress or freshen a value for period Q. Ingestion models use three-state deserialization (absent / `null` / value) for every tracked field. A source writes field F only when it **supplies** F (present in the payload, possibly `null`) and its `fetched_at` is newer than `observed_at(F)`; an absent field is never written and never touches `observed_at`. Thus a slow full response cannot overwrite a newer thin value of the same field, a thin refresh cannot make an old detail-only value look fresh, and an explicit `null` in a newer source overwrites. Field families (for TTL and reporting only): `core` (name, `due_at`, `unlock_at`, `lock_at`, `points_possible`, `html_url`), `detail` (description, submission types, allowed extensions, attempts, rubric, `can_submit`), `status` (submitted, graded, score, late, missing, excused, workflow state, attempt). The assignments list endpoint does not supply `can_submit` (only the single-assignment endpoint with `include[]=can_submit` does), so `can_submit` freshness comes only from `assignment` and `submit` pre-flight. A read that needs a `detail` field older than `ttl_assignments` reports it with `stale: true` in freshness and, online without `--offline`, refreshes it from the endpoint that supplies it: the assignments list for description-class fields, the single-assignment endpoint with `include[]=can_submit` for `can_submit`; when that endpoint is unavailable the value stays stale and `null`-safe.

A list refresh downloads every page, then in one transaction upserts entities per the rule above, **replaces the membership rows of its exact scope**, and writes `fetch_log`. Entities are never deleted by a refresh; they are unreferenced instead. A failed page leaves everything as it was and sets `stale = 1` with the error.

### Datasets

| Dataset | Scope key | Fetch | TTL | Complete when |
|---|---|---|---|---|
| `courses` | `active` \| `all` | `GET /courses` per Appendix B; `all` = three requests by enrollment state | `ttl_courses` | all pages of all requests |
| `assignments` | `course:<id>` | `GET /courses/:id/assignments?include[]=submission` | `ttl_assignments` | all pages |
| `assignment_groups` | `course:<id>:period:<id\|none>` | Appendix B, with `grading_period_id` when a period is selected | `ttl_grades` | all pages |
| `submission` | `assignment:<id>` | `GET …/submissions/self?include[]=submission_history&include[]=submission_comments&include[]=rubric_assessment` | `ttl_assignments` | one object |
| `missing` | `all` | `GET /users/self/missing_submissions?include[]=planner_overrides&include[]=course` (no `filter[]`) | `ttl_missing` | all pages |
| `planner` | `window:<start>..<end>` (UTC days) | `GET /planner/items?start_date&end_date` | `ttl_planner` | all pages; coverage = the window |
| `enrollment_grades` | `period:<id\|none>` | `GET /users/self/enrollments?type[]=StudentEnrollment&state[]=active&state[]=completed[&grading_period_id=<id>]` | `ttl_grades` | all pages |
| `course_totals` | `course:<id>` | from `courses` (`total_scores`, `current_grading_period_scores`), stored as `(course_id, all)` and `(course_id, current)` | `ttl_grades` | with courses |
| `grading_periods` | `course:<id>` | `GET /courses/:id/grading_periods` — response is `{ "grading_periods": [...], "meta": … }`, paginated | `ttl_grades` | all pages |
| `modules` | `course:<id>` | `GET /courses/:id/modules?include[]=items&include[]=content_details`, then `GET /courses/:id/modules/:mid/items?include[]=content_details` for every module whose inline `items` is absent or `null` (treated alike) or shorter than `items_count` | `ttl_modules` | all module pages and all needed item pages; per-module `items_complete` recorded |
| `folders`, `files` | `course:<id>` | `GET /courses/:id/folders`, `GET /courses/:id/files` | `ttl_files` | all pages, or a recorded denial |
| `announcements` | `window:<start>..<end>:ctx:<sha256 of sorted course ids>` | batches of ≤10 `context_codes[]` | `ttl_announcements` | all batches stored or isolated |
| `calendar_events` | `window:<start>..<end>:ctx:<sha256 of sorted contexts>` | `GET /calendar_events?type=event&context_codes[]=…` batches of ≤10 | `ttl_calendar` | all batches |
| `pages` | `course:<id>` | `GET /courses/:id/pages?sort=title` | `ttl_pages` | all pages, or a recorded denial |
| `page` | `page:<course>:<operand>` | `GET /courses/:id/pages/:url_or_id` | `ttl_pages` | one object |
| `discussions` | `course:<id>` | `GET /courses/:id/discussion_topics?only_announcements=false` | `ttl_discussions` | all pages, or a recorded denial |
| `discussion` | `topic:<id>`, and `topic:<id>:replies` with `--replies` | `GET /courses/:id/discussion_topics/:tid`; for replies `GET …/entries` and `GET …/entries/:eid/replies` | `ttl_discussions` | one object; for the replies scope, every entry page and every needed reply page |
| `inbox` | `scope:<inbox\|unread\|sent\|archived>` | `GET /conversations?scope=…&auto_mark_as_read=false` | `ttl_inbox` | all pages, or a recorded denial |
| `conversation` | `conversation:<id>` | `GET /conversations/:id?auto_mark_as_read=false` | `ttl_inbox` | one object |
| `inbox_unread` | `all` | `GET /conversations/unread_count` | `ttl_inbox` | one object |

**Hit predicate.** A dataset (or window) request is served from cache when a `fetch_log` row of that dataset and scope (for windows: with the same context hash, `window_start ≤ requested start`, `window_end ≥ requested end`) has `complete = 1`, `stale = 0`, `epoch_seen ≥ state epoch for that scope` (below), and age within TTL. Otherwise: online, refresh; if the refresh fails and a row exists, serve it with `stale: true`; `--offline` serves any existing complete row with `stale: true`, and exits 7 when none exists.

### Mutation epochs and pending writes

`state.sqlite` has `scope_epoch(scope, epoch)`. A local write (`submit` reaching any state after `planned`, `reconcile` establishing an outcome) increments, **in the same state transaction as the journal transition**, the epoch of every affected scope: `submission:assignment:<id>`, `assignments:course:<id>`, `assignment_groups:course:<id>:*`, `missing:all`, `planner:*`, `enrollment_grades:*`, `course_totals:course:<id>`. Prefix scopes (`planner:*`) are stored as prefixes and match any scope with that prefix.

An operation journal reaching `posted` or `matched` bumps its own scopes in the same transaction (§25.4):

| Operation kind | Scopes |
|---|---|
| `discussion_reply` | `discussion:topic:<tid>`, `discussion:topic:<tid>:replies`, `discussions:course:<cid>` |
| `inbox_send` | `inbox:*`, `inbox_unread:*` |
| `inbox_reply` | `conversation:conversation:<id>`, `inbox:*`, `inbox_unread:*` |

A refresh records `epoch_seen` = the state epoch read **before** its first request; at commit, inside `BEGIN IMMEDIATE` on the cache DB, it re-reads the state epoch and aborts (leaving the previous rows) if it advanced. Because the epoch lives in the durable DB and is written with the journal transition, a crash between the journal transition and any cache work cannot leave a falsely fresh cache, and `cache clear` cannot reset it.

**Pending hook.** A journal for assignment A is **pending** when its state is `planned`, `uploading`, `uploaded`, or `posting`, or when it is `outcome_unknown` and neither **superseded** nor **acknowledged**. Superseded = a journal for A created later reached `submitted` or `matched`. Acknowledged = the user ran `receipts acknowledge <journal>` (sets `acknowledged_at`; the state stays `outcome_unknown`). While any pending journal exists for A, every read that touches A reports `pending = true` and treats A's status as unknown regardless of cache age. This is evaluated at read time from `state.sqlite`. Reads never change journal state.

The hook covers operation journals too (§25.8). An operation journal is pending while its state is `planned` or `posting`, and an `outcome_unknown` one is pending until it is acknowledged; **there is no superseding rule for a write**, because a second reply or message is a second post. There are three pending targets, and each of the four §23 reads names one. `discussion@1` names its topic and reports that topic's own journals. `conversation@1` names its conversation and reports that conversation's journals **and every unresolved `inbox_send`**, because a send has no conversation id until Canvas answers, so an unresolved one may have landed in exactly this conversation. `inbox@1` and `inbox_unread@1` name the inbox and report every unresolved `inbox_send` and `inbox_reply`. Each of the four carries `pending: bool` and `pending_journals: [id]`, oldest first.

### Request budgets

Best-case baselines; `requests` in the JSON envelope reports actual counts.

| Command | Baseline API requests (fresh) | Grows with |
|---|---|---|
| `todo` | 3: courses, planner window, missing | pages; `--all` adds one `assignments` fetch per course lacking a complete list |
| `assignments <course>` | 1 | pages |
| `assignment` | 1 (+1 rubric assessment when graded) | — |
| `grades` | 2: courses, enrollments | +1 per course for the course view, +1 grading periods, +1 enrollments per explicit period |
| `download <course>` | 3 listings + item pages | +1 metadata call per file, + storage transfers |

## 11. Canvas API client (`crates/canvas-api`)

- `Client::new(origin, token: Secret, user_agent)`: `reqwest` with `rustls`, gzip and brotli for API calls, `redirect(Policy::none())`, connect timeout 10 s, API request timeout 30 s.
- **Request phases.** Every request is either an **API request** (any `/api/v1` call, including `users/self`, `Link` pagination, file-metadata calls, and upload finalization) or a **transfer request** (the multipart upload `POST` to `upload_url`, and download `GET`s). The rules differ.
- **API requests.** The bearer header is attached only when the URL origin equals the client origin. `Link: rel="next"` URLs must be same-origin; otherwise `Error::CrossOrigin`. Redirects: at most 5 hops, every hop must be same-origin (else `Error::CrossOrigin`); `303` → `GET` without body; `301`/`302` → `GET` only when the original was `GET`, otherwise `Error::UnexpectedRedirect`; `307`/`308` → same method and body. Identity (`users/self`) is accepted only from a same-origin final response.
- **Transfer requests.** The initial URL must be `https`. The token is attached only to a same-origin URL (the first download hop when the file URL is on the Canvas origin). Download hops: at most 5, `https` only, token never attached off-origin, `301`/`302`/`303`/`307`/`308` followed as `GET`. The multipart upload `POST` is **never auto-followed**: its response is consumed as the upload protocol's completion handoff (a `3xx` with a same-origin `Location` → a separate authenticated **API** `GET`; a `201` per the Upload paragraph). The stream is never replayed; any other redirect status or an off-origin `Location` is `Error::UploadIncomplete { status }`.
- `get<T>`, `get_all<T> -> Stream<Page<T>>` (`per_page=100`), `post`, `put`, `delete`. Wrapped collections (`grading_periods`) have adapters. Server validation errors surface as `Error::Validation { status, errors }`.
- **Upload.** `upload_submission_file(course, assignment, meta {name, size, content_type}, body: impl AsyncRead) -> FileId`: `POST …/submissions/self/files`; multipart `POST` to `upload_url` with every `upload_params` pair in the order received and `file` last, no token, connect 10 s and write-progress idle timeout 60 s, no total timeout; completion: on `3xx`, authenticated `GET` of a same-origin `Location`; on `201`, use the body's `id` if the body is a JSON object with one, otherwise `GET` the same-origin `Location` regardless of an empty or non-JSON body; a missing or off-origin `Location` is `Error::UploadIncomplete { status }`. The returned file ID is handed to core immediately for journaling.
- **Download.** `download(url, sink: impl AsyncWrite, expected_size: Option<u64>, on_progress) -> u64`: transfer rules above; `Accept-Encoding: identity` so byte counts are comparable; connect 10 s, idle-read 60 s, no total timeout. Responses are classified before retry: throttle (governor) first; then a Canvas-origin `401`/`403`/`404` is final and returned as `Denied { status }`; a storage `403` is returned as `StorageExpired` so the caller can refresh the URL once via `GET /files/:id`. The function validates the status, `Content-Length` when present, and `expected_size` when given; a mismatch is `Error::SizeMismatch`.
- **Throttle governor.** Separate semaphores for API (`api_concurrency`, default 4, max 8) and storage (`storage_concurrency`). The governor keeps a conservative estimate `remaining` with a timestamp. **Observation rule:** the governor keeps a nondecreasing **watermark** = the highest issue number among applied samples. A sample (`X-Rate-Limit-Remaining`) lower than the current estimate is always applied, whatever its request order, and raises the watermark only if its issue number is higher; a higher sample is applied only if its issue number exceeds the watermark (and then raises it). The watermark resets together with the estimate on the header-silence reset below. Applying an older low sample therefore never re-admits a stale high sample issued earlier than already-applied evidence. Every admitted request first subtracts its expected cost (the last seen `X-Request-Cost` for that route, default 1) from the estimate. **Refill:** the estimate grows by `refill × elapsed` between samples, where `refill` starts at 0, is set to `min(10/s, observed)` once two samples from non-overlapping requests show an increase, and is never assumed above 10/s. **Cooldown** starts when the estimate drops below 150: admissions drop to 1 in flight (in-flight requests are not cancelled). In cooldown the governor sleeps until the estimate reaches 350 or, when `refill` is 0 or the projected wait exceeds 5 s, sleeps 5 s and then admits a single **probe** request; every cooldown response updates the estimate. Cooldown ends when an applied sample is `≥ 300` (the 50-point headroom lets a charged request return a sample at or above 300). If no headers have been seen for 60 s **and nothing is in flight**, the estimate is reset to full; otherwise the last estimate is kept. Retry policy on `429` or `403` with body containing `Rate Limit Exceeded`: initial attempt plus 4 retries, delays 1, 2, 4, 8 s with ±25 % jitter, or `Retry-After` when present; retries pass through the same admission gate. Other `403`s are not retried. `X-Request-Cost` is summed into `requests.cost` and logged with `-v`.
- **Models**: `serde` with `#[serde(default)]`, `jiff::Timestamp` (offsets accepted, normalized), `jiff::civil::Date` for civil dates, `i64` IDs from numbers or strings, relative URLs resolved against the origin. Fields tracked by `field_obs` (§10) use a `Supplied<T>` = `Absent | Null | Value(T)` type with an explicit `deserialize_with` helper and a three-case fixture; every other field collapses absent and `null` to `None` (the module `items` fallback treats both alike).
- **Errors**: `Unauthorized`, `Forbidden { rate_limited, body }`, `Denied { status }`, `NotFound`, `RateLimited`, `Validation`, `CrossOrigin`, `UnexpectedRedirect`, `UploadIncomplete`, `StorageExpired`, `SizeMismatch`, `Network`, `Timeout`, `Decode`.
- **Redaction.** A `tracing` layer and the error `Display` path pass through a redactor for `Authorization`, `access_token`, every `upload_params` value, and query parameters named `Signature`, `X-Amz-*`, `Policy`, `Expires`, `verifier`, `sig`, `token`. **Response bodies are never persisted raw** by any crate; core stores allowlisted records (§12.2).

## 12. Feature specifications

### 12.1 `todo`

**Sources** (baseline 3 requests): `planner` window (default N = 14 days, `start = today − 1`), `missing`, `courses:active`.

**Kinds.** Planner `plannable_type` maps to `assignment`, `quiz`, `discussion_topic` → `discussion`, `sub_assignment` → `checkpoint`, `peer_review_sub_assignment` → `peer_review`, `assessment_request` → `peer_review`, `planner_note` → `note`, `calendar_event` → `event`, `wiki_page` → `page`, `announcement` → `announcement`; anything else → `unknown` with the raw type kept in `raw_type`. Unknown kinds are shown, never dropped.

**Keys.** Graded work with an assignment: `assignment:<assignment_id>` (planner: `plannable_id` for `assignment`, `plannable.assignment_id` for quizzes and discussions; missing: `id`). Checkpoints and peer reviews keep their own key `<raw_type>:<plannable_id>` and carry `parent_assignment_id`. Everything else: `<raw_type>:<plannable_id>`.

**Dates.** `due_at` is the assignment due time. `scheduled_at` is the planner `plannable_date` (or event start, or note `todo_date`). Day grouping uses `scheduled_at ?? due_at`. Both are output.

**Merging.** When the missing source and the planner source yield the same key: `missing = true` wins; `planner_override` fields come from the planner row; every other field follows the §10 per-field freshness rule (the newest source that supplies the field wins; `detail` fields come only from a full row and are `null` when none exists).

**Status fields.** `submitted: bool?`, `graded: bool?`, `score: number?`, `late: bool?`, `missing: bool`, `excused: bool?`, `locked: bool?`, `submittable: bool?` = `false` when a `can_submit` within `ttl_assignments` is `false`, or when `locked_for_user`, or `lock_at < now`; `true` when a `can_submit` within `ttl_assignments` is `true`; else `null`; `external: bool?` (`null` when submission types are unknown); `marked_complete: bool`, `dismissed: bool`, `pending: bool` (§10).

**Filters.** Default hides items with `marked_complete` or `dismissed`, and items that are submitted and graded — **except** items with `missing = true`, which are always shown (with a `dismissed` tag when applicable). `--missing` shows only `missing = true` items, ignoring the hiding rules. `--all` shows everything, including undated assignments; `--all` requires a complete `assignments` dataset per active course (fetched online, exit 7 offline). `--course` filters by course.

**Buckets** (shared with `assignments`): `overdue` = `due_at < now`, not submitted, not excused; `upcoming` = `due_at ≥ now`; `past` = `due_at < now`; `undated` = `due_at = null`; `unsubmitted` = not submitted and `submittable ≠ false`; `ungraded` = submitted and not graded; `future` = `unlock_at > now`; `open` = `upcoming ∪ overdue ∪ undated` (this is the single definition; §5 refers here).

### 12.2 `submit`, journal, receipts, verify, reconcile

**Authoritative record.** The `submission_journal` table in `state.sqlite` is the only authoritative record. Receipt JSON files under `receipts/` are exports rebuilt at any time by `receipts export`. Every transition is one `BEGIN IMMEDIATE` transaction that updates the row with an expected-state guard (`UPDATE … WHERE journal_id = ? AND state = ?`; zero rows affected = another actor moved it, exit 13). If the initial insert fails, the command exits 13 before any network write.

**Operation ownership and admission.** Two lock files per operation, both under `<identity dir>/journals/`, created with `create_new` semantics if absent and never deleted except by `identity remove`:

- the **admission lock** `assignment-<assignment_id>.lock`, held exclusively by `submit` from pre-flight step 2 until the journal row is published (step 7); it makes the "one active operation per assignment" check atomic across processes. The state DB additionally enforces it with a partial unique index on `submission_journal(assignment_id) WHERE state IN ('planned','uploading','uploaded','posting')`; a conflicting insert is exit 8 `in_progress`.
- the **owner lock** `<journal-id>.lock`, acquired exclusively **before** the journal row is inserted (the journal ID is generated first) and held until the journal is terminal and the receipt export has been attempted, across every network wait. Nothing else transitions a non-terminal journal while it is held.

**Owner-absent recovery** is the only other way a non-terminal journal changes state: a recoverer takes a non-blocking exclusive lock on the journal's owner lock; success proves the owner is gone; it then re-reads the row and applies the recovery table with the expected-state guard, and releases the lock. Two recoverers therefore serialize on the lock and the second one sees the terminal state. Recoverers: `submit` (for a non-terminal journal of the same assignment, under the admission lock), `submission reconcile <journal>`, and `doctor` when an identity is selected. Reads (`todo`, `receipts *`, `submission`, and every class-C command) never lock and never transition; they report `owner: live | absent | n/a` by a non-blocking probe that is released immediately.

| Found state (owner absent) | Becomes | Meaning |
|---|---|---|
| `planned` | `refused` (`abandoned before upload`) | nothing left this machine |
| `uploading` | `upload_incomplete` | an upload may be missing; nothing was posted |
| `uploaded` | `uploaded_not_submitted` | all files have IDs; the `POST` was never sent |
| `posting` | `outcome_unknown` | the `POST` was sent; its outcome was not recorded |

**Pre-flight** (fresh; `--offline` rejected):

1. Resolve course and assignment; `GET /courses/:id/assignments/:aid?include[]=submission&include[]=can_submit`. A timeout or network failure here is exit 4; no journal exists yet.
2. Take the admission lock for the assignment (non-blocking; held by another `submit` → exit 8 `in_progress`). If a non-terminal journal exists for this assignment: owner live → exit 8 `in_progress` (message names the journal); owner absent → apply the recovery table, print the recovered state, and continue with a new journal.
3. `group_category_id != null` → exit 8 (group submissions are v2). `submission_types` must contain the requested kind; `external_tool` only → exit 8.
4. Eligibility: if `can_submit` is present it governs (`false` → exit 8 with the reason Canvas gives). Otherwise: `locked_for_user`, `lock_at < now`, `unlock_at > now` → exit 8; attempts: `allowed_attempts` `null` or `-1` = unlimited; used = `submission.attempt ?? 0`; extra = `submission.extra_attempts ?? 0`; used ≥ allowed + extra → exit 8. Past `due_at` → warning only.
5. Freeze inputs. `--comment` longer than 65,535 characters is exit 2 (Canvas' text-column limit; a longer comment fails validation on the server **after** the attempt is committed). Files: read once, SHA-256, size. Text: read the bytes once (file or stdin, max 1 MiB), normalize CRLF → LF, reject empty input (exit 2), compute `input_sha256`, transform to HTML (escape `& < > "`; blank-line-separated blocks → `<p>…</p>`; single LF → `<br>`), compute `sent_sha256` of the outbound bytes; both digests and the outbound bytes are journaled. HTML: bytes as-is, `transform = "html-verbatim"`. URL: scheme `http` or `https` only.
6. Baseline: `baseline_attempt = submission.attempt ?? 0`, `baseline_submission_id`.
7. Print the plan (assignment, course, due, kind, files with sizes and hashes, estimated attempt = baseline + 1) and confirm on the terminal unless `--yes`. Then generate the journal ID, take its owner lock, insert the journal row (`planned`; the partial unique index is the last admission check), and release the admission lock.

**Journal states**: `planned` → `uploading` → `uploaded` → `posting` → `submitted` (terminal success: this process observed the `POST` response) or `matched` (terminal: Canvas shows an attempt containing exactly this operation's uploaded files; the creating request was not observed), `upload_incomplete`, `uploaded_not_submitted`, `outcome_unknown` (terminal until a new run or `reconcile`), `refused` (failure after the journal was created). Row fields: identity, course, assignment, kind, intended payload (files with hashes, text digests and outbound bytes, URL, comment), baseline attempt and submission id, timestamps per transition, uploaded file IDs (appended one per successful upload), `posting_started_at`, `post_status`, `response_kind` (`canvas-error`, `other`, `none`), `not_submitted_evidence` (`never_sent`, `assumed`), allowlisted response record with `evidence`, readback record (nullable), `server_match` record (nullable), receipt record, `acknowledged_at`, error text.

**Execution.**

8. `uploading`: files upload with concurrency 2, hashing the streamed bytes; a streamed hash that differs from the frozen hash aborts the upload and the command (`refused`, exit 8). Each file ID is committed as it arrives. A failed upload → `upload_incomplete`.
9. `uploaded` → `posting` with `posting_started_at`; then `POST …/assignments/:aid/submissions`. **Classification of the response:** only a decodable `2xx` with an `attempt` is success (step 10). **Every other outcome** → `outcome_unknown` with `post_status` (the HTTP status, or `null`), `response_kind` (`canvas-error` when the body decodes as one of Canvas' error shapes, `other` for any other HTTP response, `none` for a timeout or a dropped connection) and the sanitized error text. `response_kind` is **descriptive only**: no status, header, or body shape proves that the originating request has finished (a proxy can emit any shape while Canvas is still processing), so **the CLI never infers a negative outcome automatically** for a dispatched `POST`. Preflight and readback timeouts are not `outcome_unknown`; only the `POST` is.
9b. **Immediate resolution.** `submit` runs the reconcile procedure below once in the same run whenever a response was received, to use **positive** evidence: a newer attempt becomes `matched` or `server_match`. An empty history leaves `outcome_unknown`; the message explains that the request may still complete on the server, that `submission reconcile` re-checks, and that `--assume-not-submitted` becomes available after 30 minutes.
10. On a decodable `2xx`: hash the raw body in memory, then in **one state transaction**: commit the **allowlisted response record** with `evidence = "post-response"` (`submission_id`, `attempt`, `submitted_at`, `workflow_state`, `late`, `missing`, `excused`, `submission_type`, `attachments[] {id, display_name, size, content_type}`, `body_sha256` of the response's `body` when present, `url`, `response_sha256`); build and store the **receipt** from intent + response record with `readback = null`; set state `submitted`; increment the scope epochs (§10). A confirmed journal therefore always has a receipt.
11. Readback (enrichment): `GET …/submissions/self?include[]=submission_history`; select the history entry whose `attempt` equals the response record's `attempt`; if present, its fields are recorded as `readback` (same allowlist, plus `body_sha256` of the history entry's `body`) and the stored receipt is updated. Fields from any other attempt are never used. A readback failure leaves `readback = null` and is noted as a warning; `submission reconcile` on a `submitted` journal retries only this step (idempotent).
12. Export: the receipt is written to `receipts/<receipt-id>.json` with mode `0600`. Export failure is a warning; `receipts export` rebuilds the file from the journal row at any time. The owner lock is released after this step.

**Failure states and exits.**

| State | Meaning | Exit | Recovery |
|---|---|---|---|
| `upload_incomplete` | an upload failed; nothing posted | 9 | re-run `submit` (re-uploads; `--resume` is v2) |
| `uploaded_not_submitted` | all files have IDs and the `POST` was never sent (owner-absent recovery from `uploaded`, `not_submitted_evidence = "never_sent"`), **or** the user assumed it after 30 minutes with no attempt visible (`"assumed"`) | 9 | re-run `submit` |
| `outcome_unknown` | `POST` outcome not observed, or observed error with a matching attempt that this CLI cannot attribute | 9 | `submission reconcile <journal>` |
| `refused` | pre-flight or hash check failed after the journal existed, or abandoned in `planned` | 8 | fix and re-run |

**`reconcile <journal-id>`.** Outcomes and exits: a transition to `matched` is outcome `ok`, exit 0 (the envelope shows `attribution = "unproven"`); everything that leaves the journal unresolved is outcome `recovery`, exit 9; ineligible journals are `refused`, exit 8. **`--assume-not-submitted`** is the only way out of an `outcome_unknown` journal that never shows an attempt: allowed only after `posting_started_at + 30 min` and only when the current reconcile read shows no newer attempt; it moves the journal to `uploaded_not_submitted` with `not_submitted_evidence = "assumed"` and records the user's decision; the message states the residual risk (a re-run can create a second attempt if the original request commits later). Nothing automatic ever assumes a negative outcome for a dispatched `POST`. Eligibility: `submitted` or `matched` → idempotent: retries the readback enrichment if `readback` is `null`, prints the record, exit 0; non-terminal with a live owner → outcome `recovery`, exit 9, `in_progress`; non-terminal with an absent owner → take the owner lock and apply the recovery table first; `upload_incomplete`, `uploaded_not_submitted`, `refused` → exit 8 with the re-run hint; `outcome_unknown` → proceed under the owner lock. `reconcile` never posts. Absence is evaluated before any content or time filter. Zero entries with `attempt > baseline_attempt` and current `attempt == baseline_attempt` → stays `outcome_unknown` (outcome `recovery`, exit 9); the message says that no attempt is visible, that the original request may still complete, and that `--assume-not-submitted` becomes available after 30 minutes.

It fetches `submissions/self?include[]=submission_history` and evaluates history entries with `attempt > baseline_attempt` and `submitted_at ≥ posting_started_at − 5 min`. No server-side fact identifies which client sent a `POST`: uploaded file IDs name files in the user's submissions folder, and another client logged in as the same user could submit them first. `reconcile` therefore never claims that this process created an attempt; it records what Canvas shows.

- **File journals.** An entry whose attachment ID set **equals** the journal's uploaded ID set shows that exactly this operation's files were submitted. Exactly one such entry → state `matched`, response record filled from that entry with `evidence = "history-files"` and `response_sha256 = null`, `readback` = the same entry, receipt built with `attribution = "unproven"`, epochs incremented. Zero → stays `outcome_unknown` (`candidates` empty). More than one → stays `outcome_unknown` with every candidate listed. An entry whose attachments are copies with different IDs is not a match.
- **Text and URL journals.** `reconcile` records `server_match` = the newest candidate entry whose `body_sha256` equals the frozen `sent_sha256` (text) or whose `url` equals the frozen URL, or `null` when no candidate matches (Canvas sanitization can legitimately change a text body, so a non-matching newer attempt is listed in `candidates` but never called a match); state stays `outcome_unknown`, outcome `recovery`, exit 9. The message states that Canvas shows the listed attempts, that this CLI cannot prove it created any of them, that re-running `submit` creates a new attempt, and that `receipts acknowledge` retires the pending flag. No receipt is produced. `receipts list` shows `unknown (server match: attempt N)` or `unknown (N newer attempts, none matching)`.

**Receipt document** (`canvas-cli/receipt@1`; the same object is the export file and the `result.receipt` of `receipts show --json`):

```json
{
  "receipt_id": "…", "journal_id": "…",
  "plan_id": "…", "approval": { "channel": "tty", "at": "…", "consumer": null, "plan_sha256": "…" },
  "identity": { "origin": "https://courses.example.test", "user_id": "12345", "key": "courses.example.test-12345-3f9a1c2e" },
  "course_id": "45678", "course_code": "CHEM301",
  "assignment_id": "91011", "assignment_name": "Problem Set 3",
  "kind": "online_upload",
  "baseline_attempt": 1,
  "attribution": "observed",
  "posted": { "evidence": "post-response", "submission_id": "5551212", "attempt": 2, "submitted_at": "2026-09-10T03:12:44Z", "submitted_at_local": "2026-09-09T23:12:44-04:00", "workflow_state": "submitted", "late": false, "missing": false, "excused": null, "submission_type": "online_upload", "attachments": [ { "id": "777", "display_name": "ps3.pdf", "size": 182331, "content_type": "application/pdf" } ], "body_sha256": null, "url": null, "response_sha256": "…" },
  "readback": { "submitted_at": "2026-09-10T03:12:44Z", "submitted_at_local": "2026-09-09T23:12:44-04:00", "late": false, "attachments": [ { "id": "777", "display_name": "ps3.pdf", "size": 182331, "content_type": "application/pdf" } ], "body_sha256": null },
  "files": [ { "name": "ps3.pdf", "size": 182331, "sha256": "…", "canvas_file_id": "777" } ],
  "text": null,
  "url": null,
  "due_at": "…", "cli_version": "0.1.0", "created_at": "…"
}
```

`attribution` is `observed` (this process observed the `POST` response; `posted.evidence = "post-response"`) or `unproven` (state `matched`; `posted.evidence = "history-files"`, `response_sha256 = null`). The stored readback record keeps the full allowlist; the `Readback` object in JSON (Appendix D) is its public projection. `readback` is `null` until the enrichment succeeds. For text receipts `text = { "input_sha256", "transform", "sent_sha256", "server_body_sha256": <posted.body_sha256 ?? readback.body_sha256 ?? null> }`. A receipt is a **local integrity record**: it proves what this machine sent and what the server answered or showed for that attempt. It is not a server signature and not independent proof of deadline compliance.

**`submission verify <receipt-id>`.** Validation before any network call, otherwise `refused` (exit 8): identity matches; `posted.attempt` present; for file receipts the ID sets of `files[].canvas_file_id`, the journal's uploaded IDs, and `posted.attachments[].id` are identical and non-empty; URL receipts are refused (nothing to verify). Then it fetches `submission_history` and selects the entry with `attempt == posted.attempt`; no such entry → `unavailable` (exit 12).

- Files: the selected entry's attachment ID set must equal the receipt set, else `mismatch` listing missing and extra IDs. Each attachment is downloaded into `<identity dir>/tmp/` under §12.3 containment, hashed, and compared with `files[].sha256`. All equal → `verified` (exit 0); any difference → `mismatch` (exit 10); any attachment not retrievable → `unavailable` (exit 12) unless a mismatch was already found.
- Text: the reference is `text.server_body_sha256` (the sanitized body Canvas returned in the `POST` response or, failing that, in the readback). No reference recorded → `unavailable` (exit 12) with `reason = "no server body digest recorded"`. Otherwise the selected entry's `body` is hashed and compared: equal → `verified_body` (exit 0); different → `mismatch`; entry without `body` → `unavailable`. This checks that the stored body is unchanged; it does not re-prove the original bytes, which the receipt's `sent_sha256` records.

**`receipts list|show|export|acknowledge`** enumerate journals and receipts for the identity, newest first, with state, owner status, `server_match`, `superseded`, and `acknowledged`; `show` accepts a journal ID or a receipt ID and returns a tagged result (Appendix D); `export` rebuilds the receipt document from the journal row and writes it to a path (`--out -` streams raw JSON); it refuses journals in states other than `submitted`/`matched` (exit 8). `acknowledge` sets `acknowledged_at` on an `outcome_unknown` journal (§10 pending hook) and changes nothing else.

### 12.3 `files`, `modules`, and `download`

**Discovery** for a course: `folders`, `files`, `modules` datasets (§10). Module items of type `File` supply `content_id`, `content_details.locked_for_user`, `lock_explanation`, and the module `state`.

**Denial classification** (folders and files independently): throttle first; `401` → exit 3; `403`/`404` → the listing is recorded as `unavailable` with the status, reported as `Files listing unavailable (HTTP 403); showing files linked from modules`, and listed in `partial[]`. Listing visibility (`hidden`) and effective access (`locked_for_user`) are separate fields and are never merged.

**Coverage claim.** v1 downloads files present in the Files listing and files that are module items. Page-body and assignment-body links are v2. External-tool items are counted as `skipped_external`.

**Destination coordination.** The destination itself holds only two files under `<dest>/.canvas-cli/`, both created and opened through the retained root `Dir` handle with no-follow semantics: `dest.json` (`dest_id` uuid, identity key, format version) and `install.lock`. Identity storage holds the manifest database `<identity dir>/downloads/<dest_id>.sqlite` (§9, §10 database rules; SQLite's own path-based opens never touch the destination), the lock `<identity dir>/downloads/<dest_id>.lock`, and a `destinations` table in `state.sqlite`: `(dest_id, canonical_path, root_fingerprint, created_at)` where `root_fingerprint` is the root directory's `(device, inode)` on Unix or `(volume serial, file index)` on Windows, read through the retained root handle.

The **install mutex** is the pair of exclusive locks `<dest>/.canvas-cli/install.lock` **and** `<identity dir>/downloads/<dest_id>.lock`, always taken in that order and never held across a network transfer; the identity-side lock makes two roots that carry the same `dest_id` serialize on the shared manifest. In-process, a `tokio::sync::Mutex` serializes installs between worker tasks. Acquisition timeout 30 s → that file is `failed` (`lock_timeout`).

**Initialization**, in this order: (1) take the root lock `install.lock` (created with `create_new` through the root handle if absent); (2) read `dest.json` through the root handle, or create it with `create_new` when absent (a present but empty, partial, or unparseable file → exit 13 `destination metadata is damaged; delete <dest>/.canvas-cli to start a new destination`); (3) **identity check, on every path that read an existing file**: an identity key different from the active identity → exit 8 (`destination is bound to <origin> user <id>; choose another --dest or delete <dest>/.canvas-cli`), before any lock, registry write, manifest creation, or download; (4) take the identity-side lock for `dest_id`; (5) look up the `destinations` row. Cases: **no row and no manifest DB** for this `dest_id` → fresh destination (this also repairs a crash between writing `dest.json` and inserting the row): insert the row with the current fingerprint and path, create the DB. **No row but a manifest DB exists** → orphan: exit 13 (`unregistered destination metadata; delete <dest>/.canvas-cli to start a new destination`); ownership is never attached to an unverified root. **Row present** → fingerprint **equal** to the row → same root (a rename or move inside the same filesystem is transparent); update `canonical_path` if it changed and proceed; fingerprint **different** → unverified root, whether or not the recorded path still exists (copied metadata, or a move across filesystems): exit 8 (`this directory is not the registered destination <path>; delete <dest>/.canvas-cli to start a new destination; existing files are kept and treated as unmanaged`). No adoption workflow exists in v1. Finally open the manifest DB; a newer manifest schema → exit 13.

**Layout.** One layout; every file is written once:

```
<dest>/<COURSECODE>-<course_id>/modules/<NN>-<module-slug>/<display_name>   # module item; NN = zero-padded position
<dest>/<COURSECODE>-<course_id>/files/<folder path>/<display_name>          # only in the Files listing
```

Ownership when a file is an item of several modules: the module with the lowest `position`, then the lowest module ID. Path planning happens for the whole course before any transfer, independent of `--module` and `--file` filters, so filters never change ownership or suffixes.

**Sanitization.** Per component: replace `/ \ NUL`, control characters, and the Windows-invalid set `< > : " | ? *`; strip trailing spaces and dots; reject `.` and `..`; strip a leading `.`; a component that becomes empty is `file-<file_id>` (or `folder-<folder_id>`, `module-<module_id>`); Windows device basenames (`CON`, `PRN`, `AUX`, `NUL`, `COM0`–`COM9`, `COM¹`, `COM²`, `COM³`, `LPT0`–`LPT9`, `LPT¹`, `LPT²`, `LPT³`, with or without an extension) get a `_` suffix; truncate to 180 bytes on a UTF-8 boundary leaving room for a `-<file_id>` suffix. **Uniqueness pass:** after sanitizing every planned path for the course, compare case-insensitively after NFC normalization; every path that is not unique gets `-<file_id>` before the extension; repeat the comparison over the resulting set (generated names count as reserved) until every path is unique. The pass is deterministic for a given plan.

**Containment.** The destination root is opened once as a `cap_std::fs::Dir`. Each path component is then opened with `cap_fs_ext::DirExt::open_dir_nofollow` (created first if absent), retaining every intermediate handle; the final entry is inspected with no-follow metadata through the retained parent handle. Symlinks and reparse points anywhere below the root are refused for that file (`unsafe_path`). Final files are opened for reading (size check, hashing, move validation) through the parent handle with `FollowSymlinks::No` (`cap_fs_ext::OpenOptionsFollowExt`) and that descriptor is used for the whole validation; no operation reopens by name after inspecting. Temporary files are created in the final parent handle with `create_new` as `.<name>.<random>.part`; the install is `Dir::rename` from the parent handle to the parent handle. `--force` never changes containment.

**Manifest.** Rows: `(file_id, course_id, path, size, sha256, remote_updated_at, installed_at, pending_move_to?, move_sha256?)`, keyed by `file_id`. `sha256` is always recorded at install time from the streamed bytes (hashing is free during the transfer; `--verify` only decides whether the *local* file is re-hashed on later runs). A row proves that this CLI wrote `path`.

**Install critical section** (under the install mutex, after the transfer finished into the `.part` file): re-read the manifest row; classify the final path per the clobber table; rename; update the row in a manifest transaction committed **after** the rename; release. Crash windows: after the rename of a **new** install and before the commit, the next run finds a path with no row → `unmanaged` (the summary says to pass `--force` for that file); after the rename of a **replacement** and before the commit, the old row remains and the ordinary clobber table applies: with `--verify` (hash comparison) the file is `modified`; with the default size-only comparison an equal-size replacement counts as a local match and may be downloaded again. Neither case overwrites a file the CLI did not write. Transfers run outside the mutex, so a ready installer waits only for other installers' critical sections.

**Clobber table.** Local match = `size` equal, and `sha256` equal when `--verify`. Remote unchanged = remote `(size, updated_at)` equal to the row.

| Final path | Manifest row for `file_id` at this path | Local matches row | Remote unchanged | Action |
|---|---|---|---|---|
| absent | any | — | — | install |
| present | absent | — | — | `unmanaged`: leave intact, skip; `--force` replaces |
| present | present | yes | yes | `skipped` |
| present | present | yes | no | install (replace our own previous download) |
| present | present | no | any | `modified`: leave intact, skip; `--force` replaces |

**Move on rename.** When a file's planned path differs from its row's `path`, the run checks, under the install mutex and through no-follow descriptors: (a) the old path is a regular file whose **hash** equals the row's `sha256` (always hashed for moves, whatever `--verify` says); (b) the remote is unchanged; (c) the new path classifies as `absent` (or `--force`). All true → three durable phases, all under the mutex: (1) commit `pending_move_to = new`, `move_sha256 = sha256`; (2) `Dir::rename` old → new; (3) commit `path = new`, clear the marker. (b) false → download the new revision to the new path and leave the old file (the old row is replaced; the old path becomes `unmanaged`). (a) or (c) false → download anew; the old path is left. **Marker recovery** (at the start of every run for that destination, under the mutex, no-follow): hash whichever of the two paths exist and compare with `move_sha256`: only new matches → finalize (3); only old matches → clear the marker; both match → finalize on new and report old as `unmanaged`; neither matches (an occupied target with other bytes, or nothing) → clear the marker, keep `path = old`, and report `unresolved_move` for that file (exit 12). Size alone never decides a move.

**Transfer.** `GET /files/:id` for a fresh `url` and `size`; transfer per §11 with `expected_size`; on `StorageExpired`, refresh the URL once. Incomplete files are restarted, not resumed, in v1. Progress: one bar per active file and a total.

**Outcome mapping** (per §14): `failed`, `unavailable`, `unsafe_path`, `unresolved_move`, and `locked` → `partial`, exit 12; `unmanaged` and `modified` → exit 0 with a warning and counts unless another action forces a higher code; `--verify` `mismatch` → exit 10 (precedence over 12); `dry_run` → exit 0.

`--dry-run` prints the plan with actions, sizes, and totals. `--module TEXT`, `--file ID`, `--all-courses` as named.

### 12.4 Grades

v1 shows Canvas-reported values only.

- **`--period all`** (default when the course has no grading periods): whole-course `computed_current_*` and `computed_final_*` from `courses` (`include[]=total_scores`) via the student enrollment summaries, de-duplicated per course.
- **`--period current`** (default when the course has periods): `current_period_computed_*` from `courses` (`include[]=current_grading_period_scores`), labelled with `current_grading_period_title`.
- **`--period ID`**: `enrollment_grades` fetched with `grading_period_id=ID` (`grades.current_score`, `current_grade`, `final_score`, `final_grade`); assignment groups fetched with the same ID; the `grading_periods` dataset (wrapped, paginated) validates the ID and supplies the title. Courses that do not have that period report `unavailable`.
- Course view: groups (`name`, `group_weight`, `rules`), assignments with `points_possible`, own `score`, `grade`, `excused`, `late`, `missing`, `posted_at`, `workflow_state`, `omit_from_final_grade`; group subtotals only when the API supplies them; Canvas course totals for the selected period mode close the table. Absent or `null` totals print `unavailable` and stay `null`.
- Cache keys carry the period (§10). Totals from one period mode are never labelled with another.

**v2: local estimation.** Deferred (review B01–B03, M11–M14). The v2 contract must port Canvas' `AssignmentGroupGradeCalculator` drop selection (retained-set optimization, lowest before highest, `never_drop`, drop-count clamping, deterministic ties) and `CourseGradeCalculator` weighting (contributing groups only, no downward normalization above 100, extra credit kept, zero-denominator groups excluded); keep earned points on zero-point assignments; define eligibility from posted scores, excused, `omit_from_final_grade`, and visibility; handle grading periods; validate with fixtures that check retained assignment IDs; restrict `target` to configurations proven monotone. GraphQL is v2 and must show a measured benefit over REST.

### 12.5 `calendar` and `--ics`

**Sources.** `planner` window plus `calendar_events` (`type=event`) for contexts `user_<id>` and every active `course_<id>`, batches of ≤10, all pages. Events present in both are de-duplicated by calendar event ID; the calendar-events representation wins for event fields. Deadlines keep `due_at`; events keep `start_at`, `end_at`, `all_day`, and the civil `all_day_date`. Canvas serializes a one-day all-day event with `start_at == end_at` and does not emit the event's edit zone, so v1 treats every all-day event as **one civil day** = `all_day_date`. When `end_at ≠ start_at` the event is still emitted as one day and a warning names it (`all-day event with a longer span is shown as one day in v1`); this catches a 23-hour span across a spring DST change.

**ICS.** `--ics PATH` writes; `--ics -` streams (raw-output exception). RFC 5545: `VCALENDAR` with `PRODID`/`VERSION`; one `VEVENT` per item; `UID` = `canvas-<kind>-<id>@<identity-key>`; `DTSTAMP`; `DTSTART` in UTC, or `DTSTART;VALUE=DATE:<all_day_date>` with **no `DTEND`** for all-day events (RFC 5545 §3.6.1: a date-valued `DTSTART` without `DTEND` spans one day); `DTEND` for timed events with an end; no `DURATION` for point deadlines; `SUMMARY` = `[CODE] title`; `URL`; `DESCRIPTION` with points and status; text escaping per §3.3.11; CRLF; 75-octet folding; `VALARM` with `--alarm DURATION` (e.g. `24h`) before deadlines. M4-b manually validates import and re-import in Apple Calendar and Google Calendar (changed due date, removed event, all-day event with the profile zone west of UTC, an all-day event across a DST change, a Canvas event with equal start and end) and documents the observed behaviour. No zero-lag or auto-refresh claim.

### 12.6 `announcements`

Active courses are batched into ≤10 `context_codes[]` per request, paginated, with `start_date`/`end_date` from `--since` (default 14 days). A non-throttle `403` on a batch triggers one request per course in that batch; courses that still fail are recorded in `partial[]`. `--unread` filters locally on `read_state`. `announcement <course> <id>` (or a URL) calls `GET /courses/:cid/discussion_topics/:id`. Nothing is marked read in v1.

**v2 note (kept for the future design, not a v1 contract):** conversation detail defaults to marking a thread read (`auto_mark_as_read`), discussion views can be permission-limited and replies can require an initial post, and those routes must be specified explicitly before `inbox` or `discussions` return.

## 13. Architecture

Cargo workspace, edition 2024, MSRV **1.88**, `unsafe_code = "forbid"`.

```
canvas-cli/
  Cargo.toml
  crates/canvas-api/     # HTTP client, Secret, models, pagination, upload, download, throttle, redaction
  crates/canvas-core/    # store (cache + state), sync, resolvers, todo, journal/receipts, download, ics, markdown, io bridge
  crates/canvas-cli/     # binary: clap, renderers, JSON schema registry, config, credentials; tests/
  xtask/                 # record, sanitize, dist-assets, bench
  docs/  tasks/
```

- `canvas-api` owns `Secret` and knows nothing about disk, config, or output.
- `canvas-core` owns all three database kinds (cache, state, download manifests), locks (identity, credential, journal admission and owner, install mutex), journals, receipts, containment, and the **blocking I/O bridge**: cap-std file operations, `fsync`, hashing, HTML conversion, and credential-store calls run on `spawn_blocking` workers; file bodies cross to the async side through bounded channels of 64 KiB chunks with backpressure, without reopening ambient paths. `Store` opens without a `Client`; `Session` adds a `Client` lazily.
- `canvas-cli` owns the command enum, the JSON schema registry, and renderers.

Runtime: `#[tokio::main(flavor = "current_thread")]` plus the blocking pool above. Credential-store access happens only when a network call needs the token, except for the class-B commands listed in §5 that inspect or change the store.

Performance targets are measured by `xtask bench` (M5): cached `todo` first output p50 < 50 ms, p95 < 150 ms; full cached `todo` p95 < 250 ms; cold-start (empty page cache) p95 < 400 ms; all with a 5-course fixture and once more while one download stream is active. Numbers are recorded in `docs/bench.md`. `cargo xtask bench --watch` measures the same targets again with a resident `canvas watch` on the same identity and records one tick's cost; `cargo xtask bench --mcp` measures a warm `todo.list` round trip over a real stdio pipe and the size of the `tools/list` catalog (§21). Neither has a §13 target of its own.

## 14. Errors and exit codes

Single-invocation rule: exactly one JSON envelope (§7). Aborts use the `error` schema; completed commands with non-success outcomes use their own `result` schema with `outcome` set.

| Code | Meaning | Examples |
|---|---|---|
| 0 | Success | includes `verified`, `verified_body` |
| 1 | Generic failure | unexpected response shape |
| 2 | Usage | unknown flag; `--fresh --offline`; `--json` on a raw-output command; confirmation needed without a terminal; network command with `--offline`; `CANVAS_HOST` alone; empty `--text` |
| 3 | Auth | no token; `401`; identity mismatch; missing profile; env identity not validated offline |
| 4 | Network | DNS, TLS, timeout on an API call other than the submission `POST` (that is exit 9) |
| 5 | Rate limited | retries exhausted |
| 6 | Resolution | zero or many matches; cross-origin URL; course/URL disagreement |
| 7 | Offline miss | `--offline` and no coverage for a needed dataset |
| 8 | Refused | lock, `can_submit = false`, attempts exhausted, group assignment, disallowed extension, `external_tool` only, file changed during submit, `422`, destination bound to another identity, receipt invalid, URL receipt verify, `in_progress` journal, `reconcile`/`export` on an ineligible journal |
| 9 | Submission recovery | final journal state `upload_incomplete`, `uploaded_not_submitted`, or `outcome_unknown` (including a text/URL server match); `reconcile` on a journal with a live owner. The HTTP status of the `POST` never decides the exit or the state: a `5xx` that immediately resolves to `matched` exits 0; an error that shows no attempt stays unknown |
| 10 | Verification mismatch | `submission verify`, `download --verify` |
| 11 | Cancelled | user answered no, or Ctrl-C at a confirmation |
| 12 | Partial | some files, courses, or batches failed, were unavailable, or were refused as `unsafe_path`/`unresolved_move`; `verify` `unavailable` |
| 13 | Local persistence | DB open/migrate failure; newer schema; identity.json mismatch; lock timeout; journal insert failure; credential store `Denied`/`Locked`; logout partial failure |

**Refusal reasons.** No post-v1 package adds an exit code. Exit 8 gained a machine-readable `details.reason` on the `error@1` envelope, so an agent can branch without reading a message:

| `details.reason` | Raised by |
|---|---|
| `expired` | the plan's 15-minute admission window passed (§20) |
| `invalidated` | the plan was declined, cancelled, or is gone, or a frozen observation changed (§20) |
| `approval_required` | the plan has no recorded human approval, or its handle was rejected (§20, §21) |
| `not_attached` | nothing is attached, or the caller has not attached (§24) |
| `paused` | sharing is paused (§24) |
| `validating` | the attached page just changed and has not been confirmed (§24) |
| `account_mismatch` | the browser is signed in as another Canvas account (§24) |
| `bridge_unavailable` | no `canvas bridge host` is running (§24) |
| `stale_generation` | the named navigation generation is not the tab's current one, in either direction (§24) |
| `protocol` | a message `bridge-ipc@1` does not name, or a request over its 64 KiB bound (§24) |
| `note_too_large` | the note is over 8 KiB; nothing was held (§24) |
| `source_ref_rejected` | a note's source ref is not `canvas://` or `https` on the attached origin; nothing was held (§24) |
| `note_rejected` | the note was empty, carried more than 16 refs, or the attachment already holds its 32; nothing was held (§24) |
| `origin_mismatch` | a follow target outside the granted origin (§24) |
| `navigation_timeout` | the companion did not acknowledge a follow within two seconds; the tab may still have moved (§24) |
| `group_write` | the discussion topic is a group topic (§25) |
| `locked` | the discussion topic is locked or closed for comments (§25) |
| `initial_post_required` | the topic gates its replies and this identity has not posted (§25) |
| `unresolved` | a `--to` entry outside the topic, or a recipient id Canvas does not return (§25) |
| `empty_body` | the body is empty or only whitespace (§25) |
| `denied` | a `401`/`403` on the course, topic, or conversation at prepare (§25) |
| `unsupported` | an attachment on a discussion reply, a body over 1 MiB, or more than 10 attachments (§25) |

`zone_opaque` is the one companion reason that is **not** a refusal: the attachment is healthy, the page carries nothing, and the exit is 0 (§24.7).

`in_progress` stays a message on an exit-8 refusal, not a reason. The M8-a **read** refusals keep `code: refused` with the `initial_post_required:` message prefix and carry no reason (§23); the M8-b **write** refusal of the same gate carries `details.reason: "initial_post_required"`. Every reason above is decided before any upload, before the submission `POST`, and before any write `POST`.

**Precedence** when several apply in one invocation: an abort (2, 3, 13, 4, 5, 6, 7, in that order of detection) ends the command immediately, after the durable phase outcome (journal state, per-file results) has been committed and is carried in `details`. For a completed command: 9 > 10 > 8 > 12 > 11 > 0.

## 15. Security and compliance

- Token bound to one origin; never in the cache DB, state DB, receipts, journals, logs, or JSON. Only its SHA-256 is stored.
- No raw response bodies on disk; allowlisted records only (§12.2). Journals, receipts, and exports are mode `0600`. Fixtures pass through `xtask sanitize`, which applies the same allowlist and strips URLs.
- Everything runs locally. No telemetry, proxy, or shared server. Identifiable `User-Agent`.
- Honor `locked_for_user`, `hidden`, `unlock_at`, `can_submit`. No ID enumeration. No quiz-taking, no group submissions, no teacher endpoints.
- Governor per §11; conservative, adaptive, not a guarantee.
- Downloads and verification write only inside capability-scoped directories with no-follow traversal (§12.3).
- `cargo deny` blocks copyleft licenses and known advisories.

## 16. Testing

| Layer | Tool | What |
|---|---|---|
| `canvas-api` | `wiremock` | pagination over 3 pages; cross-origin `next` rejected; governor: delayed high sample discarded, delayed **low** sample applied, the 3-low → 1-lower → 2-high arrival order keeps cooldown (watermark), first low header enters cooldown, refill bootstrap with `refill = 0` and probe timer, absent headers ageing with and without in-flight requests, cooldown recovery under a continuous cost-1 workload (headroom), `Retry-After`, initial + 4 retries with exact delays; upload completion handoff on `3xx` and `201`; API redirects: same-origin 303/307, off-origin rejected, 301 on POST rejected; transfer redirects off-origin without token; upload with `file` last; completion on `3xx`, `201` with body, `201` empty body + `Location`, missing/off-origin `Location`; download: same-origin token, off-origin strip, `Accept-Encoding: identity`, missing `Content-Length` with wrong size, storage `403` → `StorageExpired`; redaction of every listed key; the allowlisted response record from a submission response with signed attachment URLs contains no capability-bearing URL (the submitted `url` field is retained); absent/null/value model fixture |
| `canvas-core` | `#[test]` + fixtures | todo merge (overdue graded quiz and graded discussion in both sources; dismissed missing work stays visible; unlocked assignment with `can_submit = false`; checkpoint and peer-review kinds; unknown kind retained); resolver ambiguity and incompleteness; journal state machine with failure injection after every phase incl. a kill at every lock/insert boundary and in `planned`; two simultaneous confirmations for one assignment (admission lock and unique index); a second process running `todo`, `receipts`, and `reconcile` during every active phase (no state change while the owner is live); two recoverers; owner-absent recovery per state with expected-state guards; `POST` answered by a Canvas-shaped `500` after the server committed (immediate resolution finds the attempt → `matched`); `POST` answered by JSON `400` after the server committed (comment validation → `matched`); Canvas-shaped `400` before commit (stays `outcome_unknown`, no automatic negative); branded JSON `504` with a request-id header → empty read → the original `POST` commits later → state stayed `outcome_unknown` and a later reconcile finds `matched`; timeout with zero entries stays unknown; `--assume-not-submitted` refused before 30 minutes and when an attempt is visible; login after a failed logout with the chosen store's cleanup flag set does not delete the new token; comment over 65,535 characters exits 2; crash immediately after the success transaction (receipt exists, export rebuilds); successful `POST` then failed readback (`readback = null`, reconcile enriches); another client submits the fresh upload IDs first (state `matched`, `attribution = unproven`); identical text posted by another client during `posting` (server match, stays unknown); file reconcile with 0/1/2 matching entries; unknown text journal followed by a confirmed new attempt (superseded, pending clears); `receipts acknowledge`; `reconcile` on a `submitted` journal is idempotent; identity removal with a waiting opener; scoped enrollment values P→Q→cached P incl. out-of-order P (t=30) then Q (t=20) observations on the same enrollment; out-of-order partial/full arrival per field (full t=10, thin t=30 without score, full t=20 with score → score from t=20 wins and `observed_at(score)=20`); a full refresh that omits `can_submit` does not refresh its age; fresh `can_submit=false`; `cache clear` cannot reset an epoch; crash after `submitted` before any cache work; path sanitization incl. Windows names; containment against `..`, absolute paths, in-root symlinked parent, symlinked final path, symlink swapped between inspection and open; clobber table rows incl. first-run unmanaged file and two competing installers; modified-owned file; crash after rename before commit for a new install (`unmanaged`) and for a replacement with and without `--verify`; copied `.canvas-cli` metadata between two roots (refused, also after the original root was renamed away); same-filesystem rename (transparent, `canonical_path` updated); cross-filesystem move (refused, files kept unmanaged); crash between `dest.json` creation and row insert (repaired); `dest.json` bound to identity A opened under identity B with no B registry or manifest (exit 8 before any write); `dest.json` without a row but with an existing manifest (orphan refused); damaged `dest.json`; two roots with the same `dest_id` serialize on the identity-side lock; module rename move with remote unchanged, remote changed, and target occupied; marker recovery with new/old/both/neither matching and an occupied same-size target; manifest DB newer schema refused; destination symlinked to another directory; generated-name versus literal-name collision; empty component; superscript device names; text transform (CRLF, empty, escaping); ICS escaping, folding, all-day west-of-UTC, all-day across DST, equal start/end all-day; grading periods wrapper across two pages; two-process DB access; interrupted multi-page refresh; epoch abort; newer schema refused; `cache clear` with a concurrent reader |
| `canvas-cli` | `assert_cmd` + `wiremock` + `insta` | every v1 command in table and `--json` mode at `COLUMNS=100 --color never`, `TZ` fixed, `CANVAS_NOW` frozen (test builds); every exit code in §14 incl. precedence; account switch refused; env-pair identity offline exit 3 then online validation and binding-file lookup; `auth login --profile NEW`; credential activation crash between each step (stray, pending cleanup, recovery on next login/logout); repeated fallback login with the keyring still unavailable; logout with two deletion failures leaves `active_source = none` and both flags; resolution rejects `none`; concurrent env-binding writes; class-B command with an unbound env pair and no default profile; `identity remove <key>` with no default profile; IPv6 origin identity key on Windows path rules |
| Runner | `cargo nextest` | |
| Bench | `xtask bench` | §13 targets |

Fixtures: a hand-written minimal set ships with M0-b; recordings from the owner's account via `xtask record` + `xtask sanitize` (M5) extend them. Fixtures live in `crates/canvas-api/tests/fixtures/`.

Gates for every package: `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo nextest run --all-features`, `cargo deny check`, `cargo +1.88 check --workspace --all-targets`.

## 17. Distribution

Supported routes in v1: Homebrew tap `uguryildirim24/homebrew-tap`, `cargo install canvas-lms-cli` (if published), `cargo binstall`. `cargo dist` builds `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-pc-windows-msvc`, with man pages and completions in the archives. Direct archive downloads on macOS are not a supported route until signing and notarization exist. `release-plz` manages versions and the changelog.

## 18. Milestones and work packages

Three implementation lanes (`w1`, `w2`, `w3`), one git worktree and one private `CARGO_TARGET_DIR` each. Each package has one owner for its files; every parallel round names the owner of the three shared files (command enum in `canvas-cli`, JSON schema registry, migration list). Claude merges in the stated order; the Codex reviewer gates every merge.

| # | Package | Owns | Depends on | Acceptance |
|---|---|---|---|---|
| M0-a | Workspace skeleton | everything | — | gates green incl. MSRV; §5 commands registered as stubs; `--fresh`/`--offline` conflict |
| M0-b | API contracts | `canvas-api`: `Secret`, client, request phases and redirect rules, pagination, wrapped-collection adapter, governor, redaction, **all v1 models** (users, courses, terms, enrollments, grading periods, assignments, submissions, planner, missing, folders, files, modules, module items, announcements, calendar events); hand-written fixtures | M0-a | §16 row 1 except upload and download |
| M1-a | Store core | `canvas-core::store`: cache and state DBs with the **complete v1 schema** (every §10 table incl. `field_obs`, `scope_epoch`, `credential`, `submission_journal` with its partial unique index), migrations, identity lock and `identity.json`, `fetch_log`, membership, per-field observations, hit predicate, epochs, pending-journal read hook with supersession, `Dataset` trait with a fake entity test, `cache *`; `canvas-core::identity` | M0-a | two-process, interrupted refresh, epoch abort, newer schema, `cache clear` with reader, P→Q→P scoped values with composite observation keys, out-of-order partial/full arrival, absent `can_submit` |
| M3-b-core | Download core | `canvas-core::download` planning, sanitization, uniqueness pass, containment, manifest DB (identity storage, §10 database rules, its own migration list `dl_0001`), clobber table, move protocol and marker recovery (transport behind a trait); `canvas-core::io` bridge; `canvas-core::markdown`; `canvas-core::ics` | M0-a | §16 containment, clobber, move, marker recovery, manifest schema, sanitizer, ICS rows |
| M0-c | Config, identity selection, credentials, `auth *`, `identity *`, `doctor` | `canvas-cli` config/credentials/selection; env binding file; credential table use | M0-b, M1-a | login stores under identity in one store; stray detection; `--replace`; second user refused; env-pair matrix incl. offline; keyring 4.2.0 `v1` builds on MSRV and all targets, set/get/delete on macOS; fallback file protocol incl. unsafe-file refusal; identity removal with waiter |
| M1-b | Resolvers, `courses`, `course`, `alias *`, `sync` (partial), output layer | `canvas-core::resolve`; datasets courses/terms/enrollment_grades/course_totals/grading_periods; JSON envelope, schema registry, renderers; schemas `courses@1`, `course@1`, `alias@1`, `sync@1` | M0-b, M1-a | ambiguity, incompleteness, offline; `sync` for its datasets; envelope rules |
| M2-a | Upload and download transport, journal | `canvas-api::upload`, `canvas-api::download`; `canvas-core::journal` (states, owner lock, recovery table, expected-state guards) | M0-b, M1-a | upload/download tests in §16 row 1; journal tests incl. second-process cases |
| M1-c | `todo`, `assignments`, `assignment`, `open` | `canvas-core::todo`; datasets assignments/submission/missing/planner; schemas `todo@1`, `assignments@1`, `assignment@1`, `open@1` | M1-b | §16 merge cases; baseline 3 requests on fixtures |
| M3-a | Files, folders, modules ingestion; `files`, `modules` | datasets; renderers; schemas `files@1`, `modules@1` | M1-b | inline-item completeness rule; denial classification |
| M2-b | `submit`, `submission`, `verify`, `reconcile`, `receipts *` | `canvas-core::receipts`; schemas `submit@1`, `submission@1`, `receipt@1`, `receipts@1`, `verify@1`, `reconcile@1` | M2-a, M1-c (M2-b rebases on the merged M1-c and runs its acceptance against it before merge) | failure injection per phase; admission and owner-lock cases; `5xx` after commit; text server match; file `matched`; supersession; sandbox receipt bound to the posted attempt |
| M4-a | Grades; `grades` | period-aware reads; renderer; schema `grades@1` | M1-b | period modes; wrapper pagination; null totals; duplicate enrollments |
| M3-b | `download` command | wires M3-b-core, M2-a transport, M3-a datasets; schema `download@1` | M3-b-core, M2-a, M3-a | end-to-end containment and clobber on wiremock; rerun downloads zero bytes; outcome mapping |
| M4-b | `announcements`, `announcement`, `calendar`, ICS, `sync --full` | datasets; renderers; schemas `announcements@1`, `announcement@1`, `calendar@1` | M1-c, M3-a, M3-b-core | batch isolation; ICS conformance; one-day all-day rule; manual client notes |
| M5-a | `xtask record\|sanitize\|bench`, `docs/bench.md` | `xtask` | every package through R4 | bench recorded |
| M5-b | `cargo dist`, tap, README, man pages, completions | packaging | every package through R4 | `brew install` on a clean Mac |
| M5-c | End-to-end snapshot suite for every command and exit-code precedence | `crates/canvas-cli/tests` | every package through R4 | §16 row 3 complete |

**Rounds and shared-file owners**

| Round | w1 | w2 | w3 | Enum owner | Registry owner | Migration owner | Merge order |
|---|---|---|---|---|---|---|---|
| R0 | M0-a | — | — | w1 | — | — | w1 |
| R1 | M0-b | M1-a | M3-b-core | none (no enum edits) | none | w2 (creates the full schema) | w1, w2, w3 |
| R2 | M0-c | M1-b | M2-a | w1 | w2 (creates the registry) | w3 (none expected; declares if needed) | w1, w2, w3 |
| R3 | M1-c | M3-a | M2-b | w1 | w1 | w2 | w1, w2, w3 |
| R4 | M4-a | M3-b | M4-b | w3 | w1 | w3 | w1, w2, w3 |
| R5 | M5-a | M5-b | M5-c | w3 | w3 | none | w1, w2, w3 |

An owner adds the interface (enum variant, schema entry, migration) that the other lanes need at the start of the round and pushes it first; other lanes rebase on it. A package that depends on a sibling in the same round (M2-b on M1-c) is merged last and must pass its acceptance on the merged sibling. Interface requests between lanes go through Claude.

The table above stops at R5, which is the v1 plan. The rounds after it (R6 onward) follow the same lane model; `docs/agent-ux/REPORT.md` §4 holds their package list, and Appendix C names the ones that are on `main`.

## 19. Open questions for the owner

1. ~~Can a Lasell student create a personal access token?~~ **Resolved 2026-09-09: yes.**
2. Binary name: `canvas` as specified, or a Latin name?
3. Config on macOS: `~/.config/canvas-cli` (specified) or `~/Library/Application Support`?
4. Default download destination: suggest `~/School/<term>`; confirm.
5. Fixture recording: OK to record sanitized API responses from the owner's account for tests?
6. After M5: local grade estimator, or `inbox`/`notify` first?
7. **Env override token validation (§8, raised by the M0-c code review 2026-09-09).** `credential.token_sha256` both authorizes the active stored token and records successful validation. If stored token A is active and an env token B validates for the same user, recording B there makes A unusable once the override disappears. Options: a separate validated-hash column, or an explicit exception that env overrides are never recorded. Current code: env pairs are validated per request, an identity mismatch is rejected, and the active store and hash stay unchanged (no persistent first-seen record for env tokens).
8. **`doctor --network --offline` (§5 versus §8, same review).** The `doctor` note says network checks are `skipped` with `--offline`; the class table makes `doctor --network` class D, whose offline rule is exit 2 before any I/O. Current code keeps exit 2. Decide which rule wins.
9. **`grades --period ID` without a course operand (§12.4, raised by the M4-a code review 2026-09-10).** `grading_periods` is scoped per course, so the overview cannot validate the ID or supply a title from one dataset without one request per active course (breaks the §10 budget). Current code: the ID is unknown (exit 6) only when Canvas reports no grades for it anywhere; `period.title` is `null` in the overview. Options: define the overview title as always `null`, or require a course operand with an explicit ID.
10. **`grades <course> --period current` assignment groups (§12.4, same review).** The group fetch is pinned to an ID only for `--period ID`; under `current` the course view lists every assignment and closes with the current-period total. Alternative: resolve `current` to the course's current period ID and fetch groups with it (changes the request budget and the cache scope).
11. **Overview default mode when courses disagree (§12.4 and Appendix D `grades@1`, same review).** `period_mode` is one value per run; current code picks `current` when any listed course has grading periods, so courses without periods report `unavailable`. Alternatives: `all` unless every course has periods, or a per-course mode in `grades@1`.
12. **Human date suffix on non-deadline times (§7, raised by the M2-b integration review 2026-09-10).** §7 gives one human date form with a `(in 2d 4h)` / `(overdue 3h)` suffix, which reads every date as a deadline; on a submission, comment, or history time it would print `overdue 3h` for work submitted three hours ago. Current code: due dates keep the suffix; instants (submission time, comments, history rows in `submission`) print the absolute half only via `format_local_instant`; the `receipts`, `verify`, and `reconcile` renderers still print raw `_local` strings. Decide whether §7 says the suffix applies to due dates only (then align those three renderers) or everywhere.
13. **Does `sync` refresh the per-assignment `submission` dataset? (§5 versus §10, raised by the M4-b code review 2026-09-10).** §5 lists "submissions" among what `sync` refreshes; the `submission` dataset is scoped per assignment, so warming it costs one request per assignment (about 150 for five courses of thirty), and §10's budget table says nothing about `sync`. Current code: `sync` refreshes `assignments` with `include[]=submission` only (7 requests for a one-course fixture). Reviewer's recommendation: keep that and amend §5 to say `assignments` carries submission status; otherwise add a scoping rule.
14. **`auth status` emits `pending_cleanup` (Appendix D `auth_status@1`, raised by the M5-c code review 2026-09-10).** Since M0-c the command emits a seventh field, `pending_cleanup: [string]`, which reports the `cleanup_keyring` and `cleanup_file` flags a failed `auth logout` leaves behind (§16 row 3); the Appendix D row lists six fields. The code is unchanged and the registry fixture matches it. Options: add `pending_cleanup: [string]` to the row (reviewer's recommendation: a deletion we owe is not a stray credential, and `doctor` already reports the two apart), or fold the flags into `stray_sources` and drop the field.
15. **Plan retention (§12.2 versus REPORT §3.5, raised by the M6-a code review 2026-09-10).** `plans` rows keep the full outbound bytes of every prepared submission, including declined ones; `expire` and `invalidate` change the state and leave the payload, and nothing prunes them. Before M6-a `submit` never persisted content it did not send. Decide whether expired and invalidated plans are pruned (suggested: payload cleared at the 15-minute expiry, row kept 30 days like events) and whether `doctor` reports the backlog.
16. **Blocking wait on a contended admission lock (§12.2 step 2 versus REPORT §3.5, same review).** §12.2 takes admission non-blocking and reports `in_progress`; `plan::execute` waits up to 5 s for the same plan's concurrent execute to publish its journal, then returns that journal (never a second one). A different submit still gets `in_progress` at once. Decide whether §12.2 should describe the wait or whether the two cases must be told apart without blocking.
17. **Outcome for "this plan already has a journal" (REPORT §3.2 exit table, same review).** §3.5 says a replayed acceptance returns the existing journal, but no outcome, reason, or exit is defined. Coordinator reading applied for M6-b until the owner rules: `submission.execute` on an executed plan returns the linked journal's `submit@1` envelope with that journal's own `outcome` and exit, plus `replayed: true`; the human `submit` cannot reach it.
18. **`chrono` arrives transitively through `rmcp` (Appendix A, raised by the M6-b code review 2026-09-10).** §7 timestamps use `jiff`; the workspace otherwise has no `chrono`. Accept two time libraries in the lock file as an appendix exception, or ask upstream for a feature that drops it.
19. **MCP catalog size (REPORT §3.2, same review).** `tools/list` costs about 41 900 tokens because every tool's output schema inlines the whole §7 envelope in both shapes (a host validator reads a definition on its own). `docs/bench.md` records it as the number to beat. Options: `$ref`s a host may not resolve, a smaller catalog, or a compact envelope schema per tool.
20. **`schema@1` has no registry row (REPORT §3.2, same review).** The `canvas schema` document is raw output with no §7 envelope, so `all_schemas()` cannot carry a `schema@1` row and `canvas schema schema` exits 6; REPORT §3.2's table implies a row. Decide whether the table entry is documentation only (reviewer's reading) or the document must be self-describing through the registry.
21. **Agent-surface annotations and bounds (REPORT §3.2, same review).** `download.plan` and `open.url` carry `readOnlyHint: true` because their effect is a dry run and a resolution (REPORT: annotations describe effects), though the same sentence lists downloads and navigation among non-reads; and `download.run` keeps the unbounded v1 `jobs` argument. Confirm the effect reading and say whether agent-facing `jobs` should be clamped.
22. **The coordinator opens a second `state.sqlite` connection outside the single SQLite thread (§10, raised by the M6-c code review 2026-09-10).** The shared governor row is written on the request path through its own connection so it never queues behind a command's database work; WAL and `busy_timeout` keep it correct, but a contended `BEGIN IMMEDIATE` can block the `current_thread` runtime for up to 5 s. Options: an async `GovernorState` seam, a second dedicated database thread, or amend §10's one-thread rule for the coordinator.
23. **Foreground priority is bounded per refresh, not per request (REPORT §3.6, same review).** A refresh already admitted finishes its own pagination while a submission waits; gating inside permit acquisition would park a `watch` tick while it holds the scope's single-flight lock. At `api_concurrency = 1` the residual wait is one dataset's pages. Decide whether the permit layer should learn about priority.
24. **`canvas watch` without `--since` replays the whole retained log (REPORT §3.6, same review).** Up to 30 days of events stream before live ones. The alternative is to start at the head and let a consumer ask for history with `--since 0`. Cheap either way.
25. **Cold cache plus a running `watch` can turn a foreground `--fresh` read into exit 13 (REPORT §3.6, same review).** If `watch` holds a scope's single-flight lock longer than the 30 s waiter and the cache has no complete row, the foreground reports the lock timeout instead of fetching; only `submit`/`plan execute` register interest. `todo --fresh` on a first run is the exposed case. Confirm the trade-off or let every `--fresh` read register interest.
26. **Raw HTML bodies in the cache (§15, raised by the M8-a code review 2026-09-10).** `pages.body`, `discussion_topics.message`, `discussion_entries.message`, and the v1 `announcements.message` store the Canvas HTML and convert to Markdown at read time; the syllabus converts before storing. §15 says "no raw response bodies on disk; allowlisted records only". Decide whether an allowlisted body field may be stored as sent (write it down) or every body converts at ingest (changes the v1 announcements dataset and moves the 64 KiB bound).
27. **`discussions --announcements` (M8-a contract, same review).** The pinned request `?only_announcements=false` returns non-announcement topics, so `--announcements yes` can never show one. Coordinator reading applied for the M8-a follow-on: drop the flag; `discussions` lists discussion topics only and the human output points at `canvas announcements`.
28. **`discussion --replies --page N` past the end (`discussion@1`, same review).** An out-of-range page returns `replies: []` beside `replies_coverage.complete = true`, and the schema has no window fields. Coordinator reading applied for the follow-on: add `replies_page` and `replies_total` to `discussion@1` (additive) and keep the empty page at exit 0; the owner may prefer exit 2 like `--page 0`.
29. **`submit` registers foreground interest after its resolution reads (REPORT §3.6, raised by the M6-c2 code review 2026-09-10).** §3.6 says interest is registered before the first pre-flight request, but an interest is keyed by assignment id, which is the output of the resolution read; token validation and `resolve_target` therefore run unprotected and only the eligibility read, the freeze, and the post are covered. `plan::execute` has no such window. Options: an interest not yet bound to an assignment (second lock name plus an upgrade rule), or an explicit reading that the resolution read is not a pre-flight request. Priority only, not correctness; current placement kept.
30. **The companion declares `scripting` as a third permission (REPORT §3.3, raised by the M7-a code review 2026-09-10).** Chrome requires `scripting` for `chrome.scripting.executeScript` even under `activeTab`; the alternative, a declarative `content_scripts` entry, needs `host_permissions` and injects into every Canvas page unasked. Coordinator reading applied: `activeTab`, `nativeMessaging`, `scripting`, still no `host_permissions`; recorded in `docs/companion.md`. Confirm.
31. **A broker socket client may name any consumer handle (REPORT §3.2, same review).** `attach` and `here` take the consumer as a field, so any process that can open the `0600` socket in the `0700` directory can opt in under any name. REPORT §3.2 says consumer handles express routing within the owner's OS trust domain, not isolation from another unrestricted process; the review closed the case where this leaked across the MCP adapter. State this boundary explicitly when the companion is written into the SPEC.
32. **Equal navigation generation with a different document id (REPORT §3.3 step 6, same review).** `Broker::update` refuses a lower generation; a message with an equal generation but another document id is accepted as a new document. The shipped extension increments the generation on every committed navigation, so the case is unreachable from it. Decide whether such a message is stale (refuse) or a legitimate same-document replacement (accept).
33. **`schema@1` is per-form (§7 versus `canvas schema`, raised by the M8-a3 code review 2026-09-10).** `canvas schema event` now describes the `--jsonl` line (`line`) and drops `envelope`, `result`, `error`, while every other page keeps them; the contract id stays `schema@1`. §7's bump rule governs envelopes and `canvas schema` is raw output (item 20). Reviewer's reading applied: per-form by design, because the old page described a wrapper that never exists. Decide whether that stands or the page becomes `schema@2`.
34. **`canvas notify` has no desktop backend (REPORT §3.6, raised by this consolidation pass).** §3.6 says desktop alerts consume events, and REPORT §4's M6-c row calls them optional. The command builds the alerts and deduplicates them by cursor, but `--stdout` is the only backend: `notify-rust` was rejected because its macOS path is Objective-C FFI and the workspace forbids `unsafe`. Without `--stdout` the command writes the same lines to stdout and warns on stderr, so it never claims a notification it did not post. Decide whether a real desktop backend is wanted (a separate helper, or an `unsafe` exception for one crate), or whether `notify` is a stdout producer that another tool routes.
35. **The M8-a schema pages describe nullable fields as non-nullable (§7 versus `canvas schema`, raised by the M8-a2 code review 2026-09-10).** `pages@1`, `page@1`, `syllabus@1`, `discussions@1`, `discussion@1`, `inbox@1`, `conversation@1`, and `inbox_unread@1` have no typed arm in the schema generator, so their documents are inferred from the registry fixture: `canvas schema discussion` reports `message_markdown` as `"type": "string"` although §7 lets it be `null`. The page declares `result_source: "registry fixture"`, so it is honest about being an approximation, but a strict host validating a tool result against `outputSchema` would reject a legitimate answer. Options: derive `JsonSchema` on the eight result types, or widen every inferred property. Both change the generator for other schemas too. **Resolved by 5c171ff (M8-b round): the eight result types derive `JsonSchema`, their pages declare `result_source: "result type"`, and every registered fixture is checked against its schema.**
36. **`canvas schema --list` names commands that do not exist (§5 versus `canvas schema`, same review).** The listing derives a command name from the schema id, so `conversation@1` is listed as `conversation`, `inbox_unread@1` as `inbox unread`, and `plan@1`, `receipt@1`, `reconcile@1`, `verify@1`, `event@1`, and `error@1` as commands of their own — while `canvas schema "inbox show"` and `canvas schema "inbox unread-count"` exit 6, as `submission reconcile` and `receipts verify` already did. Fixing it means giving a registry entry a command name of its own, which changes M6-b's registry contract. Current code is unchanged. **Resolved by d18bcfa (M8-b round): a registry entry carries the command that prints it, `canvas schema --list` prints command, schema, and kind, and `canvas schema "inbox show"` and `canvas schema "inbox unread-count"` now resolve; the five entries no command prints are listed as `document`.**
37. **`operation status --offline` (§5 class table, raised by the M8-b code review 2026-09-10).** The write commands are class D, and a class-D command with `--offline` exits 2 before any I/O, as `operation reconcile` does; `operation status` instead returns the stored journal (an honest local answer). Same tension as item 8. Either add an explicit exception to §5 or make `status` exit 2 and point at `receipts show` (class B). Current code kept.
38. **`operation.status` keeps `readOnlyHint: true` while it records a readback (REPORT §3.2, same review).** It moves no journal state but writes `readback_json` and can move `attribution` from `accepted` to `observed`, like every cache-backed read that writes `cache.sqlite`. Another instance of item 21. Annotation kept, description corrected.
39. **No `in_progress` refusal for a live operation target (§12.2 step 2 as the model, same review).** The operation admission lock is released once the row is published, so a second separately approved write to the same topic or conversation is admitted while the first is still `posting`; one plan still admits one journal (plan-state guard plus the unique `plan_id` index, race-tested). Reviewer's reading: a second reply is a second post, not a replacement. Record the rule in §25 or require the §12.2 refusal.
40. **`superseded` is always `false` for an operation journal (Appendix D `Journal`, same review).** A reply or message is never superseded; the column stays because `receipts list` prints one table for both journal kinds. A real superseding rule for writes would need its own definition.
41. **`background.js` does not check `sender` on `chrome.runtime.onMessage` (REPORT §3.4, raised by the M7-b code review 2026-09-10).** The `decision` and `panel_hello` branches relay whatever they are given to the host. Nothing but the side panel can reach them today (no `externally_connectable`, no page-message bridge in `content.js`), and a forged relay would still need the handle the host re-checks, so this is defence in depth, not a live hole. The reviewer did not gate them on `sender.url === chrome.runtime.getURL("src/panel.html")` because `background.js` is the one file no test loads and the package has never run in a real Chrome. Owner decision: ship the sender check after the first manual Chrome run (docs/companion.md's check table), or accept the current state.
42. **`plan::approve` treats an unparseable handle expiry as not expired (§20 handle binding, same review).** `handle_expires.parse::<Timestamp>().is_ok_and(|d| now >= d)` fails open: a row whose `expires_at` will not parse is admitted. No path writes such a row (`issue_handle` copies the `prepare`-formatted plan expiry), so it is unreachable today and it is M6-a code. Coordinator reading: a bound that cannot be read must refuse; fold into the next plan-layer change. Owner may confirm or defer.
43. **`awaiting_decision` filters handle expiry as text, not as a timestamp (§20, same review).** `h.expires_at > ?1` compares RFC 3339 strings, and jiff prints fractional seconds only when present, so `…:00.5Z` sorts below `…:00Z`. The window is under one second, it only decides whether the panel draws a row, and `approve` parses the expiry before anything moves. Tidy when the plan layer next changes; no schema or format decision now.
44. **The companion declares `sidePanel` as a fourth permission (REPORT §3.3, raised by this consolidation pass).** REPORT §3.3 names `activeTab` and `nativeMessaging`; item 30 recorded `scripting` as the third and was closed on that reading. M7-b added `sidePanel`, so the shipped manifest asks for four. It grants no host access, no tab access, and no way to read anything: the panel is a page served from the extension package and everything it shows arrives from the native host, and REPORT §3.4 forbids the alternative of a Canvas DOM overlay. `tests/companion.rs` pins the four names exactly, and `docs/companion.md` recorded both deviations. Item 30's confirmation therefore covers three of the four; confirm the fourth on the same reading, or say which surface a panel should use instead.
45. **The panel draws an operation plan without its body (§24.13 versus §25, raised by this consolidation pass).** `plan::awaiting_decision` filters by plan state and handle only, so every waiting plan reaches the panel, whichever kind it is. `PanelPlan` carries the submission half — files, `text_preview` from `payload.text`, the baseline attempt — and an operation plan stores its body, its thread, and its recipients in `operation_json` instead. Such a row therefore draws as `assignment 0` with `discussion_reply · course 101` (or `course 0` for an inbox write), its digests, and its expiry, and with no message text, no topic, and no recipients. Approving it there still works, and the host's handle, digest, and generation checks are unchanged, so this is not a forgery path. But REPORT §3.5 requires the exact bytes to be shown before an approval, and this surface cannot show them. Options: extend `PanelPlan` and the panel view with the operation block, or filter operation plans out of the panel until it can draw them. Current code is unchanged; nothing here was run in a real Chrome (§24.16).

## 20. Operation plans and approval

Built by M6-a (`docs/reviews/code-M6-a.md`) from `docs/agent-ux/REPORT.md` §3.5. A **plan** is the frozen description of one remote write, held between the moment the content is fixed and the moment a person approves it. Approval names exact bytes, not an intention.

```text
prepare  →  issue_handle  →  approve  →  execute  →  journal (§12.2)
```

`plan::execute` is the only route from a plan to a submission journal. From the journal insert onward §12.2 is unchanged. M8-b added three more plan kinds on this same layer — `discussion_reply`, `inbox_send`, and `inbox_reply` — which admit an **operation** journal instead (§25); everything in this section holds for all four kinds unless it says otherwise.

### Tables

Migration `0002_plans` on `state.sqlite` (§10).

| Table | Columns |
|---|---|
| `plans` | `plan_id` (PK), `identity_key`, `identity_generation`, `consumer`, `course_id`, `assignment_id`, `kind`, `payload_json`, `file_paths_json`, `input_sha256`, `sent_sha256`, `baseline_attempt`, `baseline_submission_id`, `observations_json`, `plan_sha256`, `state`, `created_at`, `expires_at`, `approval_json`, `journal_id`, `invalidated_reason`; index on `(assignment_id, state)` |
| `approval_handles` | `handle` (PK), `plan_id` → `plans`, `consumer`, `expires_at`, `used_at`; index on `plan_id` |
| `submission_journal` | gains `plan_id` and `approval_json`, plus the partial unique index `submission_journal_plan ON submission_journal(plan_id) WHERE plan_id IS NOT NULL` |

SQLite keeps `NULL` values distinct in a unique index, so a journal written before plans existed never collides. Such a journal exposes `plan_id` and `approval` as `null`, per Appendix D's nullable convention.

### Plan states

| State | Meaning | Reached from |
|---|---|---|
| `prepared` | frozen and stored; no approval yet | `prepare` |
| `approved` | a person approved this exact plan | `prepared` |
| `executed` | linked to a journal; says nothing about the submission's outcome | `approved` |
| `expired` | the admission deadline passed before execute | `prepared`, `approved` |
| `invalidated` | a meaningful fact changed, or the plan was declined or cancelled | `prepared`, `approved`, `expired` |

Every transition is one `BEGIN IMMEDIATE` with an expected-state guard on the row, the discipline §12.2 sets for the journal. An `executed` plan is history: `expire` and `invalidate` both name the states they may leave, so no statement rewrites it. A guard that matches zero rows is read by the transition that ran it, not by one rule: `approve` reports a lost race as a local failure (exit 13), the execute link takes it as another execute having consumed the approval and answers with that execute's journal, and `expire` and `invalidate` leave a plan they may not move untouched.

### Admission expiry

`expires_at = created_at + 15 minutes`. Expiry gates **first admission only**: a status read or a replay of a plan that is already `executed` is answered with its journal and is never turned into an expired plan. Loading a plan does not expire it.

### The approval record

```text
approval {
  channel: "tty" | "elicitation" | "panel" | "yes-flag",
  at: ts,
  consumer?: string,
  plan_sha256
}
```

`approval` is `null` before approval. `yes-flag` records an explicit CLI `--yes` and never claims an interactive decision. The audit is copied into the journal in the admission transaction and is preserved in receipt exports (Appendix D `Journal`, `receipt@1`, `plan@1`).

### The handle binding

`issue_handle` mints a random handle against a `prepared` plan and binds it to that plan, that consumer, and a deadline. `approve` spends the handle with `UPDATE approval_handles SET used_at = ? WHERE handle = ? AND used_at IS NULL` inside the same immediate transaction that sets the plan to `approved`, so a handle is single-use. Echoing a digest, a handle, or a boolean in an ordinary tool argument is not approval.

Each rejection is its own refusal, so no answer says whether another handle would have worked: `unknown_handle`, `handle_for_another_plan`, `handle_already_used`, `wrong_consumer`, `handle_expired`, `plan_digest_mismatch`.

### What `plan_sha256` covers

The digest is taken over the canonical JSON of: identity key and generation, consumer, course id, assignment id, kind, each file's name, size, and `sha256`, the text `input_sha256`, `transform`, and `sent_sha256`, the URL, a digest of the comment, the baseline attempt and submission id, every observation below, and `created_at`/`expires_at`. Object keys are sorted, so the digest does not depend on struct declaration order.

It deliberately excludes the outbound bytes and the local file paths. The bytes are pinned by `sent_sha256` and the files by their `sha256`, which §12.2 step 8 re-verifies against the streamed upload. Approving a digest therefore approves exact content, never a path that could later name other bytes.

That exclusion describes a **submission** plan. An operation plan (§25) carries its frozen write in the same canonical document, and that block holds `body.outbound_bytes` and each `attachments[].path`, so those two are inside the digest for the three M8-b kinds. Nothing weaker follows from it: the bytes and the files are still pinned by their own digests, and execute still re-hashes every attachment from disk.

### Revalidated observations

`observations_json` freezes the facts execute compares: `can_submit`, `allowed_attempts`, `extra_attempts`, `group_category_id`, `submission_types`, `allowed_extensions`, `locked_for_user`, `due_at`, `lock_at`, `unlock_at`. The two list fields are sorted, so a reordered Canvas response is not a change.

### Prepare

`prepare` runs the whole §12.2 pre-flight — step 1 fresh `GET`, step 2 admission and owner-absent recovery, steps 3–4 eligibility, step 5 freeze, step 6 baseline — and stores a `prepared` plan. It takes the admission lock for its own pre-flight only and releases it before returning: **no lock is held while a person considers a plan.** Preparing uploads nothing and posts nothing.

### Execute

In order:

1. A plan already `executed` returns its journal at once (`replayed`). Expiry is never evaluated for it.
2. Refuse an expired, invalidated, or unapproved plan, and a plan whose stored document no longer matches `plan_sha256`, before any network call.
3. Refuse a plan that belongs to another identity, or to another identity generation.
4. Register foreground interest (§22) and re-read the assignment (pre-flight step 1 again).
5. Take assignment admission. When another process holds it, wait up to 5 seconds for **this plan's own** concurrent execute to publish its journal, then return that journal; anything else is the ordinary `in_progress` refusal. (§19 item 16.)
6. Re-read the plan under admission and re-check the expiry, the invalidation, and the recorded approval, so a decline that landed during the read names itself. The digest and the identity checks of steps 2 and 3 are not repeated.
7. Compare every frozen observation against the fresh read. The first difference invalidates the plan and needs a fresh plan and a fresh approval.
8. Run §12.2 steps 3–4 (eligibility, group, kinds), then compare the baseline attempt and submission id, then the allowed extensions, then re-hash every frozen file from disk.
9. One state transaction consumes the approval, inserts the journal with `plan_id` and the approval audit, and marks the plan `executed`. Two guards make it exactly one journal per plan: the partial unique index, and `UPDATE plans … WHERE plan_id = ? AND state = 'approved'`.
10. Uploads begin only after that transaction commits.

A concurrent execute, a restarted host, and a replayed acceptance all return the existing journal. None of them creates a second attempt.

### The human `submit`

`canvas submit` keeps its v1 contract: confirmations on stderr, one `submit@1` envelope on stdout, and the §14 exit codes. It now runs on top of the plan layer — freeze, prompt, approve, execute — and pays one extra pre-flight `GET` for the revalidation read. An answered prompt records `tty`; `--yes` records `yes-flag`; a declined or unanswerable prompt cancels the plan, so it can never be executed later. The plan phase is a preview and an internal contract; it is not a second envelope.

`canvas submit` refuses a plan that already admitted a journal with exit 8. `submission.execute` replays it instead (below).

### `plan@1`

Appendix D. `submission.prepare` and the human `submit` plan phase produce it. `comment_chars` replaces the comment text, and the outbound bytes and local paths never appear.

### Exit mappings

These extend §14. No new exit code is added.

| Condition | Envelope | Exit |
|---|---|---|
| Plan expired | `outcome: refused`, `code: refused`, `details.reason: "expired"` | 8 |
| Plan invalidated, declined, cancelled, missing, or a changed observation | `details.reason: "invalidated"` | 8 |
| No recorded human approval, or a rejected handle | `details.reason: "approval_required"` | 8 |
| Another submit holds the assignment | `code: refused`, message `in_progress` (with the journal id when one is known) | 8 |
| Local lock timeout or database failure | `code: local` | 13 |

Every one of these is decided before any upload and before the submission `POST`.

### `replayed` (§19 item 17)

`submit@1` carries `replayed: bool`. `submission.execute` on a plan that is already `executed` returns that journal's own `submit@1` envelope — its own `outcome`, `state`, and `exit` — with `replayed: true`. A replayed acceptance, a second execute in flight, and a lost response all arrive there. The human `submit` never reaches that path and always reports `false`. The field is additive, so `submit@1` keeps `@1`.

## 21. Agent surface

Built by M6-b (`docs/reviews/code-M6-b.md`) and extended by M8-a2 (`docs/reviews/code-M8-a2.md`), from `docs/agent-ux/REPORT.md` §3.2. Three surfaces share one implementation: `canvas schema` publishes the contracts, `canvas mcp` serves them over the Model Context Protocol, and the shipped skill tells a model how to use them.

Every command has one core that returns the §7 envelope and a thin wrapper that prints it. `canvas mcp` calls the same core the CLI calls, so there is one implementation per command and nothing shells out to `canvas`.

### 21.1 `canvas schema`

Class A. Raw output, like `completions`: one JSON Schema document on stdout with no §7 envelope. `--json` is a usage error (exit 2). An unknown operand exits 6 and names `canvas schema --list`. `canvas schema --list` prints the registry, one row per schema shape.

The document is generated from the registry, never hand-written. A result type that derives `JsonSchema` is described exactly; the rest is inferred from the registry fixture, and `result_source` says which.

Every page carries `$schema`, `contract` (`canvas-cli/schema@1`), `command`, `schema`, `result_source`, and `output`. `output` says how the bytes are framed:

| Form | `output.form` | `output.flag` | Body of the page |
|---|---|---|---|
| Envelope | `envelope` | `--json` | `envelope` (the `oneOf` of the result and `error@1`), `result`, `error` |
| Stream summary (`watch@1`) | `envelope` | `--jsonl` | as above, plus `output.refuses: ["--json"]` |
| Stream line (`event@1`) | `jsonl` | `--jsonl` | `line` only, with its `schema` field pinned by `const`; no `envelope`, no `result`, no `error` |

A `--jsonl` line is self-describing and is never wrapped, so its page describes the line itself. The contract id stays `schema@1` for every form; §19 item 33 records that question. `schema@1` has no registry row of its own, so `canvas schema schema` exits 6 (§19 item 20).

### 21.2 `canvas mcp`

`canvas mcp` serves the Model Context Protocol on stdin and stdout. One instance serves one identity **and** one identity generation, bound at startup from the selected profile (§8). Without an identity it refuses to start with the §14 auth error, exit 3. It re-reads `identity.json` every two seconds and stops with exit 13 when the identity is replaced or removed (§10). The process writes one JSON-RPC message per line on stdout and nothing else.

**Protocol revisions.** Two are implemented, newest first:

| Revision | How it is reached |
|---|---|
| `2026-07-28` | primary; no `initialize` handshake. Every request carries its own version, client identity, and capabilities in `_meta`, and a host discovers the server with `server/discover`. |
| `2025-11-25` | through the `initialize` handshake; the adapter for hosts that have not moved yet. |

Any other revision fails explicitly with a JSON-RPC error rather than being downgraded silently.

**The tool catalog.** Exactly 43 tools, in a stable order a test pins against the report catalog. Annotations describe **effects**, not command classes: `readOnlyHint` is true only for a tool whose effect is a read, `destructiveHint` is false for every tool in the catalog, and `idempotentHint` and `openWorldHint` are set per tool.

| Effect | Tools | `readOnlyHint` |
|---|---|---|
| Read (26) | `courses.list`, `course.get`, `todo.list`, `assignments.list`, `assignment.get`, `grades.get`, `files.list`, `modules.list`, `pages.list`, `page.get`, `syllabus.get`, `announcements.list`, `announcement.get`, `discussions.list`, `discussion.get`, `inbox.list`, `inbox.get`, `inbox.unread_count`, `calendar.list`, `submission.get`, `receipts.list`, `receipts.show`, `download.plan`, `open.url`, `operation.status`, `context.here` | true |
| Local organization (10) | `sync.run`, `download.run`, `submission.prepare`, `discussion.reply.prepare`, `inbox.send.prepare`, `inbox.reply.prepare`, `context.attach`, `context.detach`, `context.note`, `context.follow` | false |
| Remote write (4) | `submission.execute`, `discussion.reply.execute`, `inbox.send.execute`, `inbox.reply.execute` | false |
| Evidence retirement (3) | `submission.reconcile`, `operation.reconcile`, `receipts.acknowledge` | false |

`download.plan` is a dry run and `open.url` resolves without launching, which is why both are annotated as reads; §19 item 21 records that reading, and §19 item 38 records the same question for `operation.status`, which records a readback while it reads. The four `RemoteWrite` tools are the four `*.execute` tools and no others, and a test pins that. `context.note` and `context.follow` are the two `Organize` tools that are not idempotent: each holds or dispatches something new.

**What the catalog does not contain.** Credentials, token reveal, identity administration, arbitrary HTTP or shell, `--yes`, cache clearing, `download --force`, and every browser action. They are unreachable by name and by argument: every argument struct rejects unknown fields, `download.*` hard-codes `dest: None` and `force: false`, `calendar.list` hard-codes `ics: None`, `open.url` never launches a browser, and `submission.prepare` hard-codes `yes: false` and refuses `text: "-"` because stdin is the transport. `download.run` keeps the v1 `jobs` argument unclamped (§19 item 21).

**Results.** A tool result carries the whole §7 envelope: `structuredContent` is the document `--json` prints, and the text block is the same document serialized. A domain failure keeps the envelope with its `outcome` and `exit`, and is marked `isError` for `error`, `refused`, `mismatch`, and `recovery`; `partial` stays a success with gaps. Only a protocol or argument failure becomes a JSON-RPC error. Every tool's `outputSchema` is the union of its result envelope and the `error@1` envelope, declared `"type": "object"` beside the `oneOf` so a strict host validator loads it.

**`ttlMs` and `cacheScope`.** The revision puts these on discovery, list, and `resources/read` results and nowhere else, so a tool result reports the same two values under this server's own `_meta` keys, `dev.canvas-cli/ttlMs` and `dev.canvas-cli/cacheScope`, rather than inventing wire fields. Every result of this server is one identity's private data, so `cacheScope` is `private` on all of them. The `ttlMs` budget is the smallest remaining TTL over every `freshness` row of the envelope, and it is **zero** for an empty list, a stale or incomplete row, an unparseable or future `fetched_at`, or a dataset with no TTL group. §7 coverage stays authoritative.

**Resources.** Namespaced by identity and generation: `canvas://<identity-key>/<generation>/<path>`. A foreign key, another generation, and an unknown path all address nothing here.

| Path | Kind | Reads |
|---|---|---|
| `todo` | listed | the default `todo.list` window |
| `receipts` | listed | local receipts and unresolved journals |
| `course/{course_id}/assignments` | template | one course's assignments, as `assignments.list` returns them |
| `context/{consumer_handle}` | template | the §24 bridge. The handle must be the reading session's own; naming another consumer's handle reads exactly what an unattached consumer reads, and an unattached consumer reads `refused` with `details.reason: "not_attached"` |

A read goes through the same command core the matching tool uses, so a resource and a tool cannot answer differently.

**The approval round trip.** The four `*.execute` tools are the only tools that can return `input_required`, and therefore the only tools whose retry may carry a `requestState`. The guard asks the catalog which tools ask, rather than naming one: a retry that names any other tool is an argument error and leaves the plan untouched. M6-b bound the state to the single name `submission.execute`; with six more approval-gated tools that would have told a host its state belonged to another tool and lost the person's decision.

1. An `*.execute(plan_id)` on a `prepared` plan issues a server-side approval handle and returns `input_required` with one keyed input request and the handle in `requestState`. Nothing is dispatched, and no client is built.
2. The host asks a person and retries the tool with a new JSON-RPC request id, the echoed `requestState`, and keyed `inputResponses`.
3. Accept spends the handle and approves the plan (§20); decline and cancel invalidate it.
4. A host that declares no elicitation gets `outcome: refused`, exit 8, `details.reason: "approval_required"`, carrying the plan id and the handle, so the approval can still be recorded through another channel.

**Subscriptions.** §22.

**Catalog size.** `cargo xtask bench --mcp` measures the catalog at **43 tools, 344 878 bytes, about 86 235 estimated tokens** per `tools/list`, against the 22 tools and about 41 900 tokens M6-b first recorded. The byte column is exact; the token column is one token per four bytes of UTF-8, a rule of thumb and not a tokenizer run. Most of each row is the output schema, which inlines the whole §7 envelope in both shapes because a host validator reads a tool definition on its own. §19 item 19 owns the question.

### 21.3 The shipped skill

`skill/canvas-cli/` ships `SKILL.md` and six workflows: `read-an-assignment.md`, `organize-the-week.md`, `download-course-files.md`, `prepare-and-submit.md`, `reconcile-an-unknown-outcome.md`, and `reply-and-message-with-approval.md`. The skill names exactly the tool catalog — a test diffs the two in both directions — and confines the two submission tools to the approval workflow and the six write tools to the sixth (§25.10). It states the envelope reading order, the §14 exit table including 11, and the coverage fields a model must not paper over (`replies_coverage`, `replies_total`, `messages_complete`, `embedded`). A forbidden flag appears only as an explicit statement that it does not exist.

### 21.4 Host matrix

`docs/agent-hosts.md` records which hosts were actually run against a build on the owner's machine, what each negotiated, and what stayed untested. It is evidence, not a claim: a host that is not in its table was not exercised. The two third-party hosts that connected both negotiated `2025-11-25`; the primary revision is exercised only by the project's own clients, and the approval round trip is verified only against the project's own client.

## 22. Coordinator, events, `watch`, `notify`

Built by M6-c (`docs/reviews/code-M6-c.md`), M6-c2 (`docs/reviews/code-M6-c2.md`), and M8-a3 (`docs/reviews/code-M8-a3.md`), from `docs/agent-ux/REPORT.md` §3.6 and §3.4. Every CLI, `watch`, and `mcp` process that binds one identity shares its network work through the identity directory and `state.sqlite`. There is no daemon.

### 22.1 The coordinator

Four shared things, all per identity:

| Purpose | Where |
|---|---|
| Cross-process request permit | `<identity dir>/locks/api-slot-<n>.lock`, one file for each `0 <= n < api_concurrency` |
| Dataset scope refresh single-flight | `<identity dir>/locks/refresh-<dataset>-<scope>.lock` |
| Foreground submission interest | `<identity dir>/locks/interest-assignment-<id>.lock` and the `interest` table |
| The shared §11 governor | the `governor` table |

Lock files are created with `create_new` when absent, locked with `fs4`, and **never deleted**; only `identity remove` removes them (§3.4). The root identity lock keeps its §9 path and is never deleted either.

**Permits.** An admitted API request holds one slot file for its whole duration, so the §11 concurrency cap is the same number whether one process or five are running, and a process that dies frees its slot with its descriptor. `flock` is per open file description, so the cap also holds between tasks inside one process. While every slot is taken the waiter polls between 2 ms and 40 ms. If the lock directory fails for longer than 5 seconds the request is admitted without a cross-process slot: a broken directory degrades to this process's own cap and says so, instead of hanging the command. Storage transfers keep a per-process semaphore.

**The shared governor row.** `governor` holds one row (`id = 1`) with the §11 `estimate`, `watermark`, `cooldown_until`, `refill`, and `updated_at`. It is read before an admission decision and merged back under `BEGIN IMMEDIATE`. Two merge rules, because §11 has two:

- **Before an admission decision**, the conservative rule applies: a lower stored estimate always wins, a higher one only above the watermark, and a live cooldown is adopted.
- **Right after this process applied a response sample**, only a strictly newer row may displace that sample. In process, §11 already lets a header replace the estimate outright; without this rule the shared estimate would fall by one pre-charge per request and never recover, and a cooldown could never end.

The per-process issue counter is realigned above any watermark this process adopts, so a process that adopts another's watermark can still apply its own later sample. The §11 header-silence reset belongs to the row, not to one process: a process that has read a live row does not reset the estimate to full from its own clock.

The coordinator reads and writes the row through its own `state.sqlite` connection, outside the store's single SQLite thread, so the request path never queues behind a command's own database work. §19 item 22 records what that costs.

**Refresh single-flight.** One process fetches a dataset scope; the others wait on its lock and then re-read the cache. The lock file name encodes the dataset and the scope: every byte outside `[a-z0-9._]` becomes `~<two lower-case hex digits>`, including `~` itself and every upper-case letter, so the encoding is injective, contains no `-` of its own, and cannot be folded by a case-insensitive filesystem. A name over 200 characters is replaced by a digest of the same two components.

A waiter gives up after 30 seconds. It then serves the coverage the cache already holds with honest §7 metadata — `source: "cache"`, `stale: true`, and the row's own `complete` and `error` — and makes no second fetch. With nothing usable it reports the §14 exit 13 lock timeout. A `--fresh` waiter accepts the holder's row only when its `fetched_at` is at or after the waiter's own start. §19 item 25 records the cold-cache case.

**Foreground interest and priority.** `plan execute` registers interest before its first pre-flight request; `canvas submit` registers it as soon as the assignment id exists, which is after its resolution reads (§19 item 29). Both hold it until the command returns. The row lives in `interest(assignment_id, kind, registered_at)` with `kind ∈ {submit, plan_execute}`; liveness is the matching lock file, so a registrant that dies frees its interest with its descriptor and polling can never be starved by a crash.

While a live registration exists, or while any journal is in state `planned`, `uploading`, `uploaded`, or `posting` (§10's pending hook), `watch` admits no new polling request and holds no slot. `watch` re-reads that state before it polls each refresh, so interest that arrives during a tick stops the rest of that tick. A terminal `outcome_unknown` journal is **not** in flight: polling continues, so its readback can still happen. A refresh already admitted finishes its own pagination; §19 item 23 records that bound, and §19 item 29 records where `submit` registers.

No network wait ever happens inside a database transaction.

### 22.2 Events

Migration `0003_events` on `state.sqlite` adds `governor`, `interest`, `observations`, `baselines`, `events`, and `consumer_cursor`. `cache clear` touches only the cache connection, so it is structurally incapable of reaching any of them.

**The observation protocol.** A cache commit and a state transaction cannot be one transaction, so the sequence is built to survive a kill between them:

1. A refresh commits its pages to `cache.sqlite`.
2. A `pending` `observations` row names the exact cache row it saw. The observation id is `<dataset>:<scope>:<fetch_log rowid>:<fetched_at>`, so a cache row that was removed or refreshed again no longer matches.
3. One state transaction compares that row against the baseline and commits the cursor, the baseline, and the events together, keyed by the observation id.

A kill between 1 and 2 leaves the baseline untouched: the next refresh reports the same difference, one refresh later. A kill between 2 and 3 leaves a `pending` row that the next `watch` tick picks up. If the cache row it named is gone or has been overwritten, the gap is real and is reported once as `resync_required`. A restart never pretends the gap did not happen, and re-applying an `applied` observation emits nothing.

**Baseline rules.**

- Only a `fetch_log` row with `complete = 1` and `stale = 0` is observed at all, so a partial or failed page can never imply a removal.
- The first complete observation of a dataset scope sets the baseline and emits nothing.
- Only the same complete scope is compared. `removed` means absent from that membership.
- `baselines(dataset, scope, observation_id, observed_at, members_json)` holds one row per dataset scope.

**Shapes.** A dataset produces events only when it has a shape, and a shape names the table, the allowlisted columns, the allowlisted `data_json` keys, and the kinds it may emit.

| Dataset | Table | Allowlisted payload | added | removed | changed |
|---|---|---|---|---|---|
| `assignments` | `assignments` | `name`, `due_at`, `points_possible`, `submitted`, `graded`, `score`, `missing`, `workflow_state`, `attempt`, `grade`, `posted_at` | `assignment.added` | `assignment.removed` | `assignment.changed` |
| `missing` | `assignments` | `name`, `due_at`, `points_possible` | `missing.new` | — | — |
| `announcements` | `announcements` | `title`, `posted_at` | `announcement.new` | — | — |
| `inbox_unread` | `conversation_unread` | `unread_count` | `inbox.unread_count` | — | `inbox.unread_count` |

Within a changed entity the fields are split: a changed `due_at` is `due.changed`; a `posted_at` that goes from null to non-null is `grade.posted`; a changed `score` or `grade` without that evidence is `grade.changed`; everything else is the shape's own change kind. A published grade is reported once, not twice.

The unread count has no removal kind and its `added` kind is unreachable: one row is the whole membership, a failed read is not observed, and a gap deletes the baseline rather than emptying it. A count that became known again is the same news to a consumer as a count that changed.

**Journal events.** `submission.state` is written inside the journal's own transaction (§12.2), with `dataset: "submission_journal"` and `scope: "assignment:<id>"`, so either both land or neither does. `operation.state` is written the same way inside an operation journal's transaction (§25.4), with `dataset: "operation_journal"`, `entity_key` the journal id, and the scope `topic:<tid>`, `conversation:<id>`, or `conversation:new`. Both carry `before`/`after` as `{ "state": … }`, and the dedupe key `operation:<journal_id>:<state>` stops a replayed execute writing a second event for a state the journal already reached.

**Plan-decision events.** A decision on a plan writes `plan.approved`, `plan.declined`, or `plan.cancelled` inside the plan transition's own transaction, with `dataset: "plans"`, `entity_key` the plan id, `before` `{ "state": "prepared" }`, `after` `{ "state": "approved"|"declined"|"cancelled" }`, and the dedupe key `plan:<plan_id>:<decision>`. The scope is `assignment:<id>` read from the plan row — `assignment:0` for an operation plan, which names no assignment — and `plan` only when the row is gone. **Nothing else is recorded**: no target, no digest, no bytes (§24.13). Invalidating a plan because a fact changed is not a decision and records no event.

**Kinds.** `assignment.added`, `assignment.changed`, `assignment.removed`, `due.changed`, `grade.changed`, `grade.posted`, `announcement.new`, `missing.new`, `submission.state`, `operation.state`, `inbox.unread_count`, `plan.approved`, `plan.declined`, `plan.cancelled`, `resync_required`.

**The log.** `events(cursor INTEGER PRIMARY KEY AUTOINCREMENT, observation_id, kind, observed_at, identity_key, generation, dataset, scope, entity_key, before, after)`. `AUTOINCREMENT` keeps `sqlite_sequence`, so retention never hands a deleted cursor to a second event — which is what a consumer's deduplication key relies on.

**Retention.** 30 days. Expiry deletes rows in one `BEGIN IMMEDIATE`; it never removes the database file.

**Cursors.** Replay is at least once, in cursor order; consumers deduplicate by cursor. A cursor below the log's low water mark, or from another identity generation, cannot be replayed: it emits one `resync_required` document and the run closes normally with exit 0. `consumer_cursor(consumer, cursor, updated_at)` holds a derived consumer's durable position; `set_consumer_cursor` never moves a position backwards, and only an unreplayable **stored** position is replaced with the log's high water mark.

**§15.** An event payload is the allowlisted cache columns and nothing else. No token, signed URL, message body, conversation subject, participant, or DOM text can reach a row.

### 22.3 `canvas watch`

`canvas watch [--jsonl] [--since CURSOR] [--once]`. Network-required (class D): `--offline` is a usage error, exit 2. `--json` is refused with exit 2 and names `--jsonl`, because the stream is its own contract and §7's one-document rule is unchanged.

`watch` is a resident consumer: it holds the shared identity lock for its whole life, so `identity remove` reports busy (§3.4).

Each tick, in order: expire the retention window; apply any observation an earlier run left pending; refresh the §10 datasets whose TTL has run out and whose backoff allows it; then emit exactly the rows the event log gained, in cursor order. Nothing is invented.

The refresh order is `courses:active`, `enrollment_grades:none`, `assignments:course:<id>` for each course, `missing:self`, `planner:default`, `announcements:courses`, and `inbox_unread:all` last. The unread count is last because it is the cheapest and least urgent dataset, so a slow inbox never delays what a deadline depends on.

Ticks are 30 seconds apart. A scope that fails backs off from 30 seconds, doubling to a ceiling of 15 minutes, so a broken scope is still retried. The TTLs do the staggering: `watch` promises no universal freshness and no 60-second guarantee (§3.6).

With no `--since`, `watch` replays the whole retained log before streaming live events; §19 item 24 records that reading. One replay read takes 500 rows.

`--jsonl` prints one complete `canvas-cli/event@1` document per line. `--once` runs one tick and closes with one `canvas-cli/watch@1` envelope reporting `since`, `cursor`, `events`, `ticks`, `resync_required`, `skipped`, and the `sync@1` dataset rows. `skipped` is `foreground_interest`, `journal_in_flight`, or `null`.

### 22.4 `canvas notify`

`canvas notify [--since CURSOR] [--stdout]`. Identity-bound and local (class B): it reads the event log only. It needs no token, opens no network connection, and refreshes no dataset.

It reads the events after its cursor, groups them by event-kind group (`assignments`, `grades`, `announcements`, `missing`, `submission`, `operation`, `inbox`, `plan`, `resync`), and writes one line per group. The position is durable in `consumer_cursor` under the consumer name `notify` and moves only **after** the lines are written, so a failed run repeats them rather than dropping them; a second run posts nothing the first one posted. Alerts are deduplicated by cursor, never by content. `--since` overrides the stored position for one run and never touches it.

`notify` is a raw-output command: it has no §7 payload, no Appendix D row, and `--json` is a usage error (exit 2).

`--stdout` is the only backend. Without it the command writes the same lines to stdout and warns on stderr that no desktop backend is available, so it never claims a notification it did not post. §19 item 34 owns that gap.

### 22.5 MCP subscriptions

`canvas mcp` implements `subscriptions/listen` over the same log (M6-c2). A host may name a cursor in `_meta` under `dev.canvas-cli/cursor`; without one, the durable consumer position is used, which is what a reconnecting host wants.

Every notification comes from the log in `state.sqlite`. There is no in-memory event source. The position advances only after a batch's notifications are sent, so a stream that dies mid-batch replays that batch rather than dropping it.

A row invalidates a resource of this binding only when it names that resource's scope — either because the scope changed, or because a `resync_required` row says an observation of it was lost:

| Dataset | Invalidates |
|---|---|
| `assignments`, scope `course:<id>` | `course/<id>/assignments` and `todo` |
| `missing` | `todo` |
| `submission_journal` | `receipts` |
| everything else | nothing |

A batch invalidates each URI once. A row from another identity key or generation invalidates nothing.

`context/<consumer-handle>` is readable but **not subscribable**: REPORT §3.2 forbids implicit sharing from resource subscriptions, and a host holding one would be told on a resync that a consumer context it does not own changed. A filter entry naming a foreign key, another generation, an unknown path, or a name no event can reach is dropped rather than refused, so the host keeps the part of its subscription that can be served.

A resync invalidates every subscribed resource once and then follows the log from its high water mark. A cursor the **host** named is the host's own position and never replaces the stored one; only an unreplayable stored position is reset.

The subscription holds a shared identity lease for the life of the stream, which makes a subscribed host a resident consumer under §3.4. It writes nothing but cursor rows and needs no token and no network. The SDK answers the first request of a connection inline, so a connection whose *first* request is `subscriptions/listen` gets no other answer while the stream is open.

## 23. Richer reads

Built by M8-a (`docs/reviews/code-M8-a.md`) and M8-a2 (`docs/reviews/code-M8-a2.md`), from `docs/agent-ux/REPORT.md` §4's M8-a row. Every command in this section is **class C**. Every request is a `GET`. Nothing here marks anything read.

### 23.1 Contract

| Command | Request | Dataset / scope / TTL |
|---|---|---|
| `pages <course> [--unpublished]` | `GET /courses/:id/pages?sort=title` | `pages` / `course:<id>` / `ttl_pages` |
| `page <course> <slug\|id\|URL>` | `GET /courses/:id/pages/:url_or_id` | `page` / `page:<course>:<operand>` / `ttl_pages` |
| `syllabus <course>` | none of its own | `course` / `course:<id>` / `ttl_courses` |
| `discussions <course> [--unread]` | `GET /courses/:id/discussion_topics?only_announcements=false` | `discussions` / `course:<id>` / `ttl_discussions` |
| `discussion <course> <id\|URL> [--replies] [--page N]` | `GET /courses/:id/discussion_topics/:tid`; with `--replies` also `GET …/discussion_topics/:tid/entries` and `GET …/entries/:eid/replies` | `discussion` / `topic:<id>` or `topic:<id>:replies` / `ttl_discussions` |
| `inbox [--scope inbox\|unread\|sent\|archived]` | `GET /conversations?scope=<scope>&auto_mark_as_read=false` | `inbox` / `scope:<scope>` / `ttl_inbox` |
| `inbox show <id>` | `GET /conversations/:id?auto_mark_as_read=false` | `conversation` / `conversation:<id>` / `ttl_inbox` |
| `inbox unread-count` | `GET /conversations/unread_count` | `inbox_unread` / `all` / `ttl_inbox` |

`per_page=100` is appended by the client to every paginated collection request whose path does not already carry it; a single-object `GET` never gets it (§11).

**Nothing is marked read.** Every conversation request carries `auto_mark_as_read=false`. The materialized `/view` discussion endpoint is never used: it marks entries read as a side effect of reading them. `--unread` filters on the stored `read_state` and `unread_count`; it marks nothing.

`sync` and `sync --full` do not refresh these datasets. They keep their v1 request budgets and their §13 targets; the new reads refresh on demand.

### 23.2 Reading rules

- **The pages listing never asks for bodies.** `include[]=body` would pull one full body per page for a listing that shows titles. A body arrives only from `page`, and the per-field write rule (§10) merges it into the same row, so a later listing refresh does not drop it.
- **`--unpublished` filters; it does not fetch.** The request is the same either way. Without the flag a page Canvas reports as `published: false` is hidden; a page with no `published` field is shown, because absence is not a denial.
- **A page is keyed by `page_id`, and its coverage scope keeps the operand the caller used.** Canvas accepts a slug and an id, and the two are different cache keys until a fetch says which page they name. Both write the same row.
- **`page` and `discussion` need their course operand even for a URL.** A URL naming a different course than the operand is exit 6, and so is a URL on another origin.
- **`syllabus` sends no request of its own.** It reads the `course` dataset. The cache stores `syllabus_markdown` and the JSON `syllabus_refs` projection beside it, never the source HTML. A cache written before M8-a has no projection, so its reference lists are empty rather than wrong.
- **`syllabus.updated_at` is the course's own `updated_at`.** Canvas reports no revision time for a syllabus body, so the field is `null` today rather than a guess.
- **`discussions` lists discussion topics only.** The request is pinned at `only_announcements=false`, so Canvas never returns an announcement through it. There is no `--announcements` flag (§19 item 27), and the human output points at `canvas announcements`. `is_announcement` stays in `discussions@1` and `discussion@1`, because Canvas sends it on a topic: it is a topic property, not a filter.
- **Replies have their own coverage scope.** A read without `--replies` covers `topic:<id>`; with `--replies` it covers `topic:<id>:replies`. One can never make the other look covered. Without `--replies` the answer is `replies: []` with `replies_coverage { pages_fetched: 0, complete: false, blocked: "not_requested" }` — never `complete`.
- **Nested replies are followed only when Canvas truncated them.** An entry carries its `recent_replies` inline; only an entry with `has_more_replies` costs a second request. `pages_fetched` counts every page across both routes.
- **A reply-page failure keeps what was stored.** Fetching stops at the first failure, the pages already read are ingested, and the topic row records `replies_complete = false` with `replies_blocked`. The coverage lives on the row, so a later cached read reports the same incompleteness and the same exit.
- **`--page N` selects which stored replies to show, 100 per page.** The fetch still covers the whole set. A page past the end is exit 0 with an empty `replies` list, not an error (§19 item 28). `discussion@1` carries `replies_page` (1 when `--page` is absent) and `replies_total`, so an empty window is never read as a thread with no replies. `replies_total` is `null` when `--replies` was not asked: no count was made.
- **`discussion@1` replies carry `parent_id`.** The reply list is flat and holds both top-level entries and nested replies.
- **A conversation listing row is not a conversation.** `inbox show` reports `messages_complete`, which is false when only the listing row is cached, so an empty `messages` array is never read as "no messages".
- **A conversation message body is plain text.** Canvas sends it as text, not HTML, so it is not converted to Markdown. It is bounded like every other body.
- **The unread count may be unknown.** Canvas returns it as a string or a number. A value that is absent or unparseable stays `null` rather than becoming `0`. The count lives in a one-row table, because the cache is per identity.

### 23.3 Bodies, embedded content, and file references

One HTML body is converted to Markdown and a `BodyRefs` projection. The projection holds no HTML.

- Every `<iframe>`, `<video>`, `<audio>`, `<embed>`, and `<object>` becomes an `embedded` row and a one-line placeholder in the Markdown, so a reader is told what it cannot see. `kind` is `video`, `audio`, `lti` (an `<iframe>` whose source names `external_tools` or `/lti/`), `iframe`, or `unknown`. `reported` is always `unavailable`; nothing embedded is fetched.
- Every `<a href>`, `<area href>`, `<img src>`, and `<source src>` is resolved against the active identity origin at read time. A same-origin reference whose path ends in `/files/:id` becomes a `files` row; every other origin becomes an `external_links` row and is never fetched. A fragment, a `mailto:`, and a `javascript:` URL are dropped.
- **Every reference is stripped of its capability-bearing parts before it is stored or shown** (§15): the userinfo, and every query parameter §11's redaction list names — `access_token`, `verifier`, `sig`, `token`, `Signature`, `Policy`, `Expires`, `X-Amz-*`. The rest of the reference is kept as written, because that is what tells the reader where it points. The rewrite happens before the Markdown is rendered, so a converted body carries no capability either. A persisted `html_url` keeps only its origin and path, the rule `modules` already used.
- **A body over 64 KiB is cut on a character boundary.** `truncated` is `true`, the envelope gains a `partial[]` row, and the exit is 12. A cut body is never reported as complete. The bound counts per document: a page, a syllabus, a topic message, one reply, and one conversation message each count on their own.

Raw HTML bodies stay in `pages.body`, `discussion_topics.message`, and `discussion_entries.message` and are converted at read time, following the v1 `announcements.message` pattern; the syllabus converts before storing. §19 item 26 owns that difference.

### 23.4 Exits

| Case | Outcome |
|---|---|
| Listing denied (`403`/`404`, not a throttle) | `partial[]` row `pages:course:<id>`, `discussions:course:<id>`, or `inbox:scope:<scope>`; `listing.available = false`; exit 12. The denial is stored as coverage, so a later cached read reports it too. |
| Single item denied | exit 8, `code: refused` |
| Not found | exit 6, through the resolver path |
| Cross-origin URL operand | exit 6, `code: resolution` |
| `require_initial_post` gate with `--replies` | exit 8, `code: refused`, message starting `initial_post_required` |
| A reply page failed after earlier pages were stored | `replies_coverage.complete = false`, `blocked: "page_failed"`, `partial[]` row `discussion_entries:topic:<id>`, exit 12 |
| A body cut at 64 KiB | `truncated: true`, `partial[]`, exit 12 |
| `--offline` with no complete coverage | exit 7 |
| Bad `--scope`; `--page` without `--replies`; `--page 0` | exit 2 |

A `403` on the entries route of a topic whose `require_initial_post` is true is the initial-post refusal, and only when `--replies` was asked; reading the topic itself still succeeds. A `403` on a topic without that flag is an ordinary incomplete page set. Auth (`401`) and throttling (`429`, or a rate-limited `403`) always propagate as exit 3 and exit 5; they are never recorded as coverage.

### 23.5 The rubric extension

Additive on `assignment@1` and `submission@1`. A rubric criterion keeps `id`, `description`, and `points`, and gains `long_description`, `criterion_use_range` (false when Canvas does not say), and `ratings[]` (empty when Canvas does not send one), each rating being `{ id, description, long_description?, points? }`. A rubric assessment gains `rating_id`, `null` when Canvas does not name the rating. Both are re-projected on read, so a row cached by an earlier build still carries every declared field. Fixtures written before M8-a still validate, and both schemas keep `@1`.

### 23.6 Cache and config

Cache migration `0002_reads` adds `pages`, `discussion_topics`, `discussion_entries`, `conversations`, and `conversation_unread`, and moves `CACHE_USER_VERSION` to 2. Each table is keyed by its Canvas id, as the v1 tables are. Nested arrays — `group_topic_children`, conversation `participants` and `messages` — live in `data_json`; reply entries live in `discussion_entries` with membership under `discussion_entries` / `topic:<id>`.

`cache.ttl_pages` (default `1h`), `cache.ttl_discussions` (default `15m`), and `cache.ttl_inbox` (default `5m`) join the §9 keys.

### 23.7 Schemas

`pages@1`, `page@1`, `syllabus@1`, `discussions@1`, `discussion@1`, `inbox@1`, `conversation@1`, and `inbox_unread@1` are registered with fixtures (Appendix D). Ids are strings, every declared field is present, unknown values are `null`, and an array is never `null`. Text bodies are Markdown, bounded at 64 KiB per document.

These eight schemas now have typed arms in the schema generator, so their `canvas schema` pages are derived from the result types and describe a nullable field as nullable. They declare `result_source: "result type"` rather than `"registry fixture"`. §19 item 35 records the gap and the commit that closed it.

## 24. Companion, broker, presence

Built by M7-a (`docs/reviews/code-M7-a.md`) and M7-b (`docs/reviews/code-M7-b.md`), from `docs/agent-ux/REPORT.md` §3.3 and §3.4. `canvas-cli` reads the Canvas page a person already has open. It does that with a Chrome extension attached to one tab and a broker process Chrome starts on the same machine. **There is no cookie import, no token in the browser, and no fetch proxy.** The browser never becomes a way to reach Canvas: the one request the companion makes is a fixed account probe, and everything else it reports is the page it was given.

```text
the tab a person attached          this machine
 content script  ── bridge-native@1 ──  canvas bridge host  ── bridge-ipc@1 ──  canvas here
                                                                                canvas mcp (context.*)
```

Nothing is shared until the person acts. `activeTab` grants one tab for as long as it stays on the origin the gesture happened on; a cross-origin navigation revokes the grant, and with it the attachment.

### 24.1 The extension

`extension/` ships in the release archives and is loaded unpacked. Manifest V3, `minimum_chrome_version` 116, and **exactly four permissions**:

| Permission | Why |
|---|---|
| `activeTab` | one tab, granted by the gesture, revoked by a cross-origin navigation |
| `nativeMessaging` | the pipe to `canvas bridge host` |
| `scripting` | Chrome requires it for `chrome.scripting.executeScript` even under `activeTab` (§19 item 30) |
| `sidePanel` | the panel is a page of this extension; it reaches no site (§19 item 44) |

There is no `host_permissions`, no `content_scripts`, no `externally_connectable`, and no `cookies`, `webRequest`, `declarativeNetRequest`, `webNavigation`, `history`, `tabs`, `storage`, `downloads`, or `debugger`. `tests/companion.rs` asserts every one of those against the shipped manifest, and asserts that no file the panel page loads contains `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`, `eval(`, `new Function`, or `srcdoc`. The package has no dependency: `cd extension && npm test` installs nothing.

`activeTab` is the whole access model. The extension is injected only by `chrome.scripting.executeScript` inside a gesture — a click on the toolbar action or `Alt+Shift+C` — and only into the tab that gesture named. A declarative `content_scripts` entry would need `host_permissions` and would inject into every Canvas page unasked.

**The lifecycle.** A gesture on an unattached tab opens the side panel, connects the native port, injects the isolated-world scripts, and offers one observation. A second gesture on the same tab detaches: the toolbar button is the way to stop sharing as well as to start. A committed navigation of the attached tab increments a **navigation generation** when the origin is unchanged and ends the attachment when it is not. Sharing pauses on a hidden tab after `bridge.pause_hidden_after`, on entering an assessment, on a tab close, and on host loss. Every message the extension sends carries the tab id and the navigation generation the service worker holds.

The service worker performs no fetch and reads no page. It relays; it composes nothing that the panel shows, and the panel's one message back — a decision on a plan — travels to the host unchanged, for the host to check.

### 24.2 Zones

The content script classifies the document **before** it reads any of it. The route gives one classification, the frames on the page give another, and the stricter of the two wins.

| Zone | What it is | What leaves the browser |
|---|---|---|
| `open` | an ordinary course page, assignment, discussion, page, or module listing | route, sanitized URL, title; text only when a consumer asks |
| `graded` | a gradebook or submission view | the same. Grades are read from the API, never scraped |
| `assessment` | a quiz or graded assessment | nothing at all |
| `external` | an LTI or external-tool frame | nothing at all |
| `unknown` | a frame this build does not recognize | nothing at all |

`assessment`, `external`, and `unknown` are the **opaque** zones. For them the bundle carries no route ids, no URL, no title, and no text, and `content_reason` is `zone_opaque`. Entering an assessment also pauses sharing, before anything is asked for.

A frame is read as attributes only — id, name, src, title, class — and never by first reading its contents. A frame naming `quiz`, `assessment`, `exam`, `proctor`, or `lockdown` makes the page `assessment`; one naming `tool_content`, `lti`, `external_tool`, or `basic_lti` makes it `external`; `preview_frame`, `wiki_page_show`, and `speed_grader_iframe` are Canvas' own and stay `open`; **anything else makes the whole page `unknown`**, because the safe reading of a frame nobody recognizes is that it might be an assessment.

Both ends classify. The extension classifies from the document it can see; the host classifies again from the sanitized URL it was sent and takes the stricter reading, so the extension cannot talk the host into a more permissive zone. An opaque page sends no URL at all, so the host has no path of its own to read: an already-opaque classification stands, and anything else reads `unknown` (M7-a defect 3).

Routes are recognized from numeric ids only, and the page kinds are `dashboard`, `course`, `assignment`, `announcement`, `discussion`, `quiz`, `grades`, `modules`, `files`, `page`, `calendar`, and `other`.

### 24.3 The account probe

The companion makes exactly one Canvas request, and it makes it in the isolated-world content script, where it is same-origin and needs no host permission of any kind: `GET <origin>/api/v1/users/self`, with `redirect: "error"`, `credentials: "same-origin"`, `cache: "no-store"`, and `Accept: application/json`. A redirect is a login wall or another origin and is a failure, not an answer. The response is reduced to `{ user_id, observed_at }` **inside the content script**: no name, no email, no login id, no avatar leaves the page.

The broker joins browser and API facts only when that probe's `user_id` equals the active identity's user id. A mismatch erases the stored observation, pauses the attachment, and refuses with `account_mismatch`. A failed probe releases no text.

### 24.4 Text release

Metadata is served from the broker's own memory. It triggers no probe and no page read.

Text — the selected passage and the visible editor excerpt — is released only when a consumer asks for it (`canvas here --text`, or `context.here` with `include_text: true`), and only after the extension probes the account again and the broker verifies it against the identity. An opaque zone releases none.

Hidden inputs, credential-looking field names, and capability-bearing query parameters are stripped before anything leaves the page. The stripped parameters are `verifier`, `signature`, `token`, `access_token`, `sig`, `policy`, `expires`, and `session_token`, matched case-insensitively, plus every parameter whose name begins `x-amz-`. A sanitized URL keeps http and https only and loses its fragment, its username, and its password. The whole payload is bounded at **64 KiB of UTF-8**, cut on a character boundary, with `truncated: true` when it was cut.

### 24.5 The native host and the broker

`canvas bridge host` is the broker. Chrome starts it as `canvas chrome-extension://<id>/`, and `canvas` recognizes an extension origin in that position and runs the host. It is class B in every respect that matters — it opens no network connection and needs no token — and it is the one raw-output command that speaks a binary framing (§7).

`serve()` runs in this order, and refuses at the first step that fails:

1. **Check the caller origin.** A missing origin, a non-extension origin, and an extension id other than `bridge.extension_id` are each refused with exit 8. An extension id is 32 characters from `a`–`p`.
2. **Take the shared §10 identity lock**, so `identity remove` sees the host as a live consumer.
3. **Take the ownership lock** `<data root>/bridge/<identity-key>.lock`, exclusively, for the host's lifetime. A second host for the same identity reports the first one's pid and start time and exits 8.
4. **Clear a stale socket**, but only while holding that lock, so a live endpoint is never unlinked by a newcomer.
5. **Bind and restrict the endpoint**, then send `ready`.

The host re-reads `identity.json` every two seconds. If the identity is gone or was replaced, it stops. The same watchdog pushes a fresh panel state whenever the event log has moved.

**Endpoint.** `<data root>/bridge/<identity-key>.sock` inside `<data root>/bridge/`, directory mode `0700` and socket mode `0600`. On Windows it is the named pipe `\\.\pipe\canvas-cli-<identity-key>` with the DACL `D:P(A;;GA;;;{owner SID})(A;;GA;;;SY)` — the owner and `SY`, nobody else. The socket path is length-checked against the 103-byte `sockaddr_un` bound before the bind, and an overrun is a named local failure that says to set `CANVAS_DATA_ROOT` somewhere shorter, not an opaque `bind` error.

**Identity removal.** `identity remove` sends `release` on the socket **before** it takes the identity lock exclusively. The host detaches, tells the extension, and exits. If it has not let go within five seconds, `identity remove` reports the identity busy, names `canvas bridge detach`, and changes nothing. After a successful removal `identity remove` unlinks the socket and the broker's own ownership lock file — the one exception to §10's never-delete-a-lock rule, and the reason §9 marks that row as removed by `identity remove`. The root identity lock and the coordinator locks stay, as §10 requires.

### 24.6 `bridge-native@1`

Chrome's native-messaging framing: a 4-byte **native-endian** length, then UTF-8 JSON. A message over 1 MiB is refused from the length alone, before any buffer is allocated for it.

| Extension → host | Host → extension |
|---|---|
| `hello` — protocol, extension id, browser-profile instance | `ready` — protocol, identity key, origin, `pause_hidden_after_ms` |
| `attach` — one observation | `attached` — the attachment state |
| `update` — a navigation, a visibility change, a re-probe | `request_text` — ask for the selection and the excerpt |
| `text` — the answer, with a fresh account probe | `refused` — a named reason |
| `pause` / `detach` — with a cause | `detach` — sharing is over |
| `navigate_ack` — the companion took a navigation, or refused it | `navigate` — go to this URL, inside the granted origin |
| `navigate_outcome` — what became of it, later | `note` — one note for the panel to display |
| `decision` — the person approved, declined, or cancelled a plan | `panel` — everything the panel shows |
| `panel_hello` — the panel opened, or reloaded | |

A pause carries its cause: `hidden`, `assessment`, `tab_closed`, `user_detached`, or `cross_origin`.

**What `Broker::update` does with an observation.** A different origin ends the attachment. A different tab is a protocol error. A different browser-profile instance ends it. A **lower** navigation generation is `stale_generation`. An unverified account erases the stored observation, pauses, and refuses. `zone == assessment` erases and pauses. A new document id moves the attachment to `validating`, and nothing is released until the extension confirms it. An **equal** generation with a different document id is accepted as a new document; §19 item 32 owns that reading. One identity holds one attachment: a newer tab replaces an older one.

### 24.7 `bridge-ipc@1`

Newline-delimited JSON on the endpoint above. A request line over **64 KiB** is refused as it arrives. `MAX_REQUEST_BYTES` is deliberately larger than the 8 KiB note bound, so a note at its own limit comes back as `note_too_large`, which a person can act on, rather than `protocol`, which nobody can.

| Operation | Who may call it |
|---|---|
| `attachments.list` | anyone. It carries no attachment id and no page content |
| `attach` | a consumer, naming itself. It receives the attachment id and the state, never the page |
| `here` | a consumer that attached, or the CLI when there is one attachment. The caller is checked **before** the browser is asked for anything |
| `detach` | a named consumer gives up its own share; the CLI ends the attachment |
| `release` | `identity remove`, before it takes the exclusive lock |
| `note` | a consumer that attached, or the CLI. It holds one note for display and writes nothing |
| `follow` | the same callers. It asks the browser to move the tab inside the granted origin |

**There is no approval operation on this socket.** A plan is approved over the native-messaging path, from the panel, and nowhere else. Anything that reaches this socket has, by construction, nothing to say about a plan; asking it to approve answers `refused: protocol`, and a test drives that path.

**Refusal reasons.** Every one is exit 8 except `zone_opaque`, which is exit 0.

| `reason` | Meaning |
|---|---|
| `not_attached` | nothing is attached, or the caller has not attached |
| `paused` | sharing is paused |
| `validating` | the page just changed and has not been confirmed |
| `zone_opaque` | the attachment is healthy and this page carries nothing. **Not a refusal**; exit 0 |
| `account_mismatch` | the browser is signed in as another Canvas account. Nothing was joined |
| `bridge_unavailable` | no host is running |
| `stale_generation` | the named generation is not the tab's current one, in either direction |
| `protocol` | a message this protocol does not name, or a request over its bound |
| `note_too_large` | the note is over 8 KiB. Nothing was held |
| `source_ref_rejected` | a source ref is not `canvas://` or `https` on the attached origin. Nothing was held |
| `note_rejected` | the note was empty, carried more than 16 refs, or the attachment already holds its 32. Nothing was held |
| `origin_mismatch` | the follow target is outside the granted origin |
| `navigation_timeout` | the companion did not acknowledge within two seconds. The tab may still have moved |

`not_attached` and the rest are the `details.reason` values §14 records.

### 24.8 Consumers and the trust boundary

A consumer is a name: `mcp` for an anonymous MCP client, `mcp:<client name>` when the client identifies itself, and the CLI when no name is given. `attach`, `here`, `note`, `follow`, and `detach` take the consumer as a field, so **any process that can open the `0600` socket in the `0700` directory can call under any name.** REPORT §3.2 says consumer handles express routing within the owner's OS trust domain, not isolation from another unrestricted local process, and that is the boundary this release implements. §19 item 31 states it.

What the boundary does hold is everything above the socket:

- **A consumer handle is set by the adapter, never by a model.** `context.here` from `canvas mcp` always carries the calling host's own handle, so one MCP consumer cannot read another's attachment even with a stolen attachment id.
- **`attachments.list` carries no attachment id and no page content.** The attachment id is the capability, so a listing must not hand it out. `bridge status` therefore names the state, the origin, the account id, the zone, the consumers, and the generation, and never the page.
- **`context.detach` gives up only the caller's share.** Ending the attachment for everyone is `canvas bridge detach`, a human act.
- **`context.attach` returns the handle and the state, never the page.** Opting in and reading are two decisions.

### 24.9 Commands

Every command here opens the session locally and reaches Canvas only where §5's class says so.

| Command | Class | What it does |
|---|---|---|
| `bridge install [--extension-id ID] [--browser chrome\|chromium\|edge]` | B | writes the native-messaging host manifest, mode `0600`, and stores the id in `bridge.extension_id` |
| `bridge host [CALLER_ORIGIN] [--parent-window HANDLE]` | B | the broker. Chrome starts it; raw output (§7) |
| `bridge status` | B | the manifest, the owner, and the attachments |
| `bridge detach [--attachment ID]` | B | asks the live owner to end the attachment |
| `here [--attachment ID] [--text]` | C | the context bundle: the browser side, and the API side for what its route names |
| `note --text T [--source-ref REF]… [--attachment ID] [--generation N]` | B | holds one inert note for the panel |
| `open <target> --follow [--attachment ID]` | B | navigates the attached tab instead of opening a window |

`bridge install` writes this file to the browser's `NativeMessagingHosts` directory — `~/Library/Application Support/{Google/Chrome|Chromium|Microsoft Edge}/NativeMessagingHosts/` on macOS, `~/.config/{google-chrome|chromium|microsoft-edge}/NativeMessagingHosts/` on other Unix:

```json
{
  "name": "com.canvas_cli.bridge",
  "description": "canvas-cli companion broker",
  "path": "/absolute/path/to/canvas",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://<ID>/"]
}
```

Windows registers native messaging hosts in the registry, which `canvas bridge install` does not write: it refuses with exit 8 rather than guessing. Without an id and without `bridge.extension_id`, the command is a usage error, exit 2.

`canvas here` is **class C**, not class B: its browser half comes from the socket, and its API half calls the same `course`, `assignment`, and `announcement` cores the CLI calls, each of which may fetch. Those envelopes travel whole, with their own freshness. `bridge install|status|detach`, `note`, and `open --follow` reach the socket only and are class B.

### 24.10 The side panel

The panel is a page of the extension. It opens on the same gesture that shares the tab — opening it needs that gesture — and it shows four things: what is attached, the plans waiting for a decision, the notes an agent left, and the journals for the page the person is on.

It has **no model and no chat backend**, it opens no database, and it makes no request of its own — not to Canvas, not anywhere. Everything it draws arrives from `canvas bridge host` as one `panel` message. The service worker relays that message and composes none of it.

The API side travels as whole §7 envelopes, the same documents `canvas course --json` and `canvas assignment --json` print, each keeping its own freshness. **The host reads them offline, and only offline.** A panel is redrawn whenever the event log moves or the person opens it, and a surface that fetched on every redraw would make the browser the reason Canvas is called. A row the cache has not refreshed is shown as stale, and a fact the CLI does not hold is shown as not held — never borrowed from the browser observation beside it, which is a hint and not a fact. There is no page text in any of it.

**The status feed.** The panel is one more consumer of the §22 event log and follows its cursor rules. A position the log can no longer replay is not quietly restarted: the panel says **refresh** and starts again. It draws at most 20 journals. Journal and receipt states are shown with their exact §12.2 names, and the sentence beside a name explains it rather than replacing it: `matched` says the attribution is unproven, `outcome_unknown` says the outcome was never observed, and nothing is drawn as finished except `submitted`, which is the one state in which this machine saw the response that created the attempt.

### 24.11 Notes

`canvas note --text …`, or `context.note` from an agent, holds one note for the attachment. A note is inert:

- **it is not a Canvas write.** Nothing about it reaches Canvas;
- **it is not an approval.** No note, whatever its text or its refs say, can approve, decline, or cancel a plan;
- **it is not HTML.** The text travels as Markdown source and the panel renders a small subset — headings, paragraphs, lists, quotes, code, bold, italic, links — as elements built one at a time with their text set as text. There is no HTML parser in that path, so a tag stays a tag on screen; no image is ever fetched; and a link survives only when it is `https` on the granted origin with no credentials in it. Everything else is shown struck through and marked *(link removed)*, so the reader sees that something claimed to be a link rather than seeing a link that lies.

Bounds, and what breaking one costs:

| Bound | Value | On breach |
|---|---|---|
| Note size | 8 KiB | `note_too_large`; the whole note is refused, never truncated |
| Source refs | 16 per note | `note_rejected` |
| Notes held per attachment | 32 | `note_rejected` — the thirty-third is refused, and no older note is pushed out |
| Emphasis nesting | 4 deep | rendered as text past the bound |
| Block-quote nesting | 6 deep | rendered as text past the bound |

Each ref is either `canvas://…`, non-empty and free of control characters, or an `https` URL whose origin equals the granted origin **and which carries no username and no password**: `Url::origin()` ignores credentials, so `https://you@canvas.example/…` has the right origin and reads as another host to a person. The nesting bounds exist because each level is a recursive call and a note well inside 8 KiB can otherwise carry enough of them to overflow the stack (M7-b defect 2). A note that still fails to render costs that one row, which is shown unrendered; the plans waiting for a decision draw either way.

Notes are held for the attachment's lifetime, survive a navigation and a pause, and are erased on detach. Every panel push carries every held note, which is the second reason the count is bounded: a native message stops at 1 MiB, and a host that cannot send a message treats the pipe as lost.

### 24.12 Follow

`canvas open <target> --follow`, or `context.follow`, asks the attached tab to go somewhere inside the granted origin. The target goes through the ordinary `canvas open` resolver first, so a target outside this identity's Canvas fails at exit 6 and never reaches the browser. `may_follow` is checked before anything is dispatched.

The answer is a **dispatch acknowledgement**: the companion took the request. It is not a page load. What became of the page arrives later and lands on `here@1` as `browser.follow.load` — `loaded` or `unknown`. **This build never reports `failed`**: seeing a load failure needs `webNavigation`, which would grant standing visibility of every navigation in the browser. Ten seconds after a navigation with nothing observed, the answer is `unknown`, which is what is actually known. A late outcome that names a request the bundle no longer reports changes nothing.

A follow belongs to the consumer that asked for it: one agent's navigation is not reported to another as its own.

**Navigating is not a preview.** A preview promises to change nothing; handing a URL to a browser promises no such thing, because Canvas' own page controllers run and the discussion controller marks a topic read when it renders it. `follow@1` says so in `side_effects`, and `canvas open --follow` says so on stderr.

**Generations bind in both directions.** A note or a follow naming a generation **behind** the tab is stale; one naming a generation **ahead** of it is `stale_generation` too, because an agent that names a generation the tab has not reached is working from something other than what it read. The CLI may omit `--generation`, because a person typing a note is looking at the tab; the agent tools may not, because an agent works from a bundle it read earlier.

### 24.13 Panel approvals

When a plan is waiting for a decision, the panel shows the frozen plan — the files with their sizes and hashes, a bounded text preview of at most 512 characters, the digests, the baseline attempt, and when the plan expires — and offers **approve**, **decline**, and **cancel**. Those three words are the only decisions `Decision::parse` accepts.

The decision travels from the panel to the host over native messaging, carrying the handle the panel was shown. The host then checks, in order and before anything moves:

1. the handle is one it issued for that plan and is still unspent, **compared in constant time**, and the echoed handle *selects* a stored row rather than supplying one;
2. the digest the panel echoed is that plan's;
3. the stored plan still describes itself (`plan.digest() == plan.plan_sha256`);
4. the plan belongs to the current identity generation.

Only then does it call `plan::approve` with `channel = "panel"` (§20), which checks the handle, the consumer, and the expiry again inside its own transaction. Each rejection is its own refusal: `unknown_decision`, `unknown_plan`, `not_prepared`, `bad_handle`, `plan_digest_mismatch`, `stale_generation`, and `refused` when the plan layer itself refuses.

The panel draws **every** plan waiting for a decision, whichever kind it is, and it draws the submission half of one: an operation plan (§25) shows its kind, its digests and its expiry, but no body, no thread, and no recipients, because those live in the plan's operation block and `PanelPlan` does not carry it. §19 item 45 owns that gap.

A page script cannot reach any of this. It runs in the extension's own surface, and the socket a page might reach through a consumer names no approval operation at all. §19 item 41 records that `background.js` does not yet check `sender` on the relay, which is defence in depth rather than a live hole.

**Approval events are private.** The log records `plan.approved`, `plan.declined`, or `plan.cancelled` with the plan id and the decision, and nothing else — no target, no digest, no bytes (§22.2). Invalidating a plan because a fact changed is not a decision and records no event.

### 24.14 The agent surface

Five tools join the §21.2 catalog, and one resource template becomes readable.

| Tool | Effect | `idempotentHint` |
|---|---|---|
| `context.attach` | Organize | true |
| `context.here` | Read | true |
| `context.detach` | Organize | true |
| `context.note` | Organize | false |
| `context.follow` | Organize | false |

`context.note` and `context.follow` require `generation`; `attachment_id` is optional on all five, and every argument struct rejects unknown fields.

`canvas://<identity-key>/<generation>/context/{consumer_handle}` reads the bundle the calling session's own handle names. Reading it attaches nobody: until that consumer calls `context.attach` the resource answers `not_attached`. **The handle in the URI must be the reading session's own.** Naming another consumer's handle reads exactly what an unattached consumer reads — the same `here@1` refusal, the same reason, the same exit — so the resource never reports whether that other handle attached, or even whether a broker is running. It stays readable but not subscribable (§22.5).

**Browser context is never cacheable.** `ttl_ms` is `0` and the bundle carries no `freshness` row of its own, so nothing downstream can serve a stale observation as a fact. The API and browser sides stay separate documents: `api` holds whole §7 envelopes, each with its own freshness, and a browser extract never updates one of them.

### 24.15 Schemas

`here@1`, `note@1`, `follow@1`, and `bridge@1` are registered with fixtures (Appendix D). `bridge@1` has three variants — `status`, `install`, and `detach` — one schema for three shapes. `follow@1` has no command name of its own, because `--follow` is a flag on `open`.

### 24.16 What has been run, and what has not

This is the record, and nothing here claims a flow that was not executed.

**Run, against the shipped host.** `tests/bridge.rs` and `tests/m7b.rs` start the real `canvas bridge host` the way Chrome starts it and drive it with hand-written native-messaging frames. Between them they cover the `ready` handshake; `hello` and `attach`; `bridge status` and `here --json` over the real socket; a wrong extension id, a non-extension origin, and no origin at all, each exit 8; a stale socket cleared under ownership and a live endpoint never unlinked; a second host reporting the first; `identity remove` completing after the cooperative release, with the ownership lock gone and the root identity lock untouched; notes and follows bound to the generation in both directions; a stale and a cross-origin follow that never reach the browser; a dispatch acknowledgement separated from its load outcome, and a late outcome for a replaced navigation changing nothing; approve, decline, and cancel through the panel path, each with its recorded event; and every forgery path — a note shaped like an approval, the socket asked to approve, a guessed handle, the digest used as a handle, a rewritten digest, another plan's id, and a fourth decision word — each refused with the plan still `prepared`, followed by the real decision working. One assertion reads every byte that crossed either pipe and fails on a token or cookie name, with a capability parameter and a planted secret put on a URL the companion reports so the assertion has something to catch.

Two consumers are exercised over a real `canvas mcp`: only the consumer that attached reads the bundle, a stolen attachment id does not serve the other one, reading the other consumer's resource by name does not either, and one consumer letting go leaves the tab attached for the person. `canvas-core::bridge` unit-tests the framing bounds, account mismatch, a failed or redirected probe, stale document and navigation generations, opaque zones, and the 64 KiB ceiling. `cd extension && npm test` runs the route classifier, the zone classifier, the sanitizer, the byte bounds, the note renderer, and the panel view model against fixture HTML and hostile fixture notes, importing the shipped files rather than a copy.

**Not run: a real Chrome.** The companion has never been loaded into a browser on the owner's machine. Installing the manifest writes into the user's own Chrome profile directory, and both the probe and the API side need a real Canvas account. So the following are untested and unverified: load unpacked, the toolbar gesture, a same-origin navigation keeping the grant, a cross-origin navigation revoking it, an account switch producing `account_mismatch`, two tabs, a broker restart driven by Chrome, the side panel opening on the gesture, a note rendered on screen, hostile markup proving inert in a real document rather than in the node tree the tests read, `chrome.tabs.update` moving a tab, a `loaded` outcome from a real navigation, the approve, decline, and cancel buttons, and a forged approval refused with a real page in the tab. Every Chrome-facing rule above rests on Chrome's documentation plus the tests, not on observation. `docs/companion.md` carries the check table for running them.

**Not run: Windows.** The named pipe and its SDDL descriptor compile and unit-test on every platform, but no pipe was created and no registry key was written.

**Measured.** `cargo xtask bench --bridge` starts a real host and times a warm metadata `here` over the socket at p50 0.026 ms against a 100 ms target, and a follow acknowledgement at p50 1.234 ms against a 300 ms target; the answer is 673 bytes on the wire. `docs/bench.md` holds the run.

## 25. Discussion and inbox writes

Built by M8-b (`docs/reviews/code-M8-b.md`) from `docs/agent-ux/REPORT.md` §3.5 and §4's M8-b row. Three remote writes join `submit`: a discussion reply, a new conversation, and a message added to a conversation. Every one is **class D**, runs `prepare → approve → execute` on the §20 plan layer, and lands in an **operation journal** that keeps the §12.2 discipline with the submission target replaced by an operation target.

**Nothing is dispatched without a recorded human approval, nothing is ever resent, and nothing claims more than it observed.**

### 25.1 Contract

| Command | Request on execute | Admission lock |
|---|---|---|
| `discussion reply <course> <topic\|URL> [--to ENTRY_ID] (--text T \| --text-file P \| --text -) [--attach P]… [--yes]` | `POST /courses/:cid/discussion_topics/:tid/entries` with `message`, or `POST …/entries/:eid/replies` with `--to` | `journals/topic-<tid>.lock` |
| `inbox send --to USER_ID[,…] [--subject S] (--text …) [--attach P]… [--yes]` | `POST /conversations` with `recipients[]`, `subject`, `body`, `group_conversation=false`, `attachment_ids[]` | `journals/conversation-new-<plan-id>.lock` |
| `inbox reply <conversation_id> (--text …) [--attach P]… [--yes]` | `POST /conversations/:id/add_message` with `body`, `attachment_ids[]` | `journals/conversation-<id>.lock` |
| `operation status <journal_id>` | the readback below; no write | owner lock only |
| `operation reconcile <journal_id> [--assume-not-posted]` | the readback below; no write | owner lock only |

Prepare reads, all `GET`, all before anything is frozen:

| Purpose | Request |
|---|---|
| the topic, to check the gates | `GET /courses/:cid/discussion_topics/:tid` |
| a `--to` entry, to check it is in the topic | `GET /courses/:cid/discussion_topics/:tid/entries` |
| the conversation, for `inbox reply` | `GET /conversations/:id?auto_mark_as_read=false` |
| each recipient id | `GET /search/recipients?user_id=<id>` |

The readback both `operation status` and `operation reconcile` run:

| Kind | Request |
|---|---|
| `discussion_reply` without `--to` | `GET /courses/:cid/discussion_topics/:tid/entries` |
| `discussion_reply` with `--to ENTRY_ID` | `GET /courses/:cid/discussion_topics/:tid/entries/:eid/replies` |
| `inbox_reply`, and `inbox_send` once Canvas has named a conversation | `GET /conversations/:id?auto_mark_as_read=false` |

A threaded reply is read on the replies route because that is where Canvas puts it: the topic's entry listing is top-level only, so reading it would report every threaded reply as absent. `auto_mark_as_read=false` is on every conversation read, as in §23: reading to check a write must never mark it read.

`operation reconcile --offline` is exit 2, the class-D rule. `operation status --offline` returns the stored journal instead — an honest local answer that reads nothing; §19 item 37 owns that difference.

### 25.2 The plan

Three plan kinds join `submission`: `discussion_reply`, `inbox_send`, and `inbox_reply`. They share the §20 plan layer unchanged — the 15-minute admission expiry, the channels `tty`, `yes-flag`, `elicitation`, and `panel`, the handle binding, the digest the approval is bound to, and the rule that one plan admits at most one journal.

A plan freezes:

- the exact thread: `course_id` and `topic_id`, with `parent_entry_id` when `--to` was given, or `conversation_id`, or the exact recipient ids;
- the subject, for a send;
- the body, as `input_sha256` over the normalized input (CRLF folded to LF, so the digest does not change with a `--text-file`'s line endings) and `sent_sha256` over the outbound bytes, with the transform named;
- every attachment as name, size, SHA-256, and absolute path.

Execute re-hashes every attachment from disk. Changed bytes are `invalidated`, exit 8, and nothing is sent. Limits: **1 MiB** of body text, the same `MAX_TEXT_BYTES` `submit` uses, and at most **10 attachments**.

Storage. An operation plan lives in the same `plans` table. `course_id` and `assignment_id` stay `NOT NULL` there and hold `0` for a write that names no course and no assignment; `plans.operation_json` carries the real target, and `plan@1` prints `null` for the fields that mean nothing. The submission half of the row is empty: no frozen files, no baseline, no observations.

**The body transform.** `discussion_reply` sends `text-to-html`: the text is escaped and wrapped in `<p>`/`<br>` by the same `text_to_html` a text submission uses, because Canvas renders a discussion `message` as HTML, so a `<` the person typed is never markup. Both inbox writes send `plain`: Canvas treats a conversation `body` as plain text and escapes it itself, so escaping here would show the person their own entities.

**What the digest covers.** `plan_sha256` is taken over the canonical plan document, which for these kinds includes the frozen operation. §20's rule that the digest excludes the outbound bytes and the local file paths holds for a submission plan; an operation plan's canonical document carries `operation.body.outbound_bytes` and each `operation.attachments[].path`, so approving one binds the exact bytes and the exact paths as well. Both are pinned by their digests too, and execute re-verifies every attachment from disk before anything is uploaded.

### 25.3 Refusals at prepare

Every one is exit 8 with `result.details.reason`, and every one happens **before any `POST`**.

| `reason` | When |
|---|---|
| `group_write` | the topic has a `group_category_id`, or a non-empty `group_topic_children` |
| `locked` | the topic is `locked` or `locked_for_user` |
| `initial_post_required` | `require_initial_post` is set and this identity has not posted |
| `unresolved` | a `--to` entry outside the topic, or a recipient id Canvas does not return |
| `empty_body` | the body is empty or only whitespace |
| `denied` | a `401`/`403` on the course, topic, or conversation |
| `unsupported` | an attachment on a discussion reply, a body over 1 MiB, or more than 10 attachments |

A cross-origin URL is exit 6 (`resolution`), not exit 8: it is not a Canvas object this identity was refused, it is not this identity's Canvas at all.

**The initial-post gate is never opened.** The tool posts no placeholder to reveal a thread, and the skill tells a host not to either.

A discussion reply cannot carry an attachment in this version. Canvas takes one on the entries route, but freezing and uploading it needs the entry-scoped upload route; it is refused at prepare rather than dropped silently.

### 25.4 The operation journal

Migration `0004_operations` on `state.sqlite` (§10) adds `operation_journal`, its unique index on `plan_id`, indexes on state and course, and the nullable column `plans.operation_json`. `STATE_USER_VERSION` moves to 4.

States: `planned → posting → posted | matched | outcome_unknown | refused | failed`. `posted` and `matched` are the two ways an operation ends as done, `refused` is the terminal for one that was never sent, `failed` for one Canvas answered with a non-2xx, and `outcome_unknown` for one whose outcome was never observed.

The discipline is §12.2's:

- the **admission lock** is held across the insert, so two executes never publish a journal for one target at the same moment. It is released once the row exists. Unlike a submission — where §12.2 step 2 refuses `in_progress` while a live journal holds the assignment — a second **separately approved** write to the same topic or conversation is admitted while the first is still `posting`, because a second reply is a second post, not a replacement. Two executes of the *same* plan are still exactly one journal, by the plan-state guard and the unique index on `plan_id`. §19 item 39 owns that rule;
- an **owner lock** is held for the whole operation;
- every transition is a guarded `UPDATE … WHERE state = ?`, so a lost race changes nothing;
- the row update, the `operation.state` event, the receipt, and the cache epochs commit in **one transaction**.

`inbox_send` locks on the plan id, because until Canvas names a conversation there is no target to lock. `conversation-new-<plan id>` still stops one plan from being admitted twice, and does not stop a person from writing to two people at once.

**Execute, in order.** A plan already executed returns its journal at once. Then: the admission guard; the recorded approval; the digest re-check; the identity key; the identity generation; a **revalidation** that re-reads the target and re-applies every §25.3 refusal; the admission lock by target name, where a contended lock waits up to 5 seconds for this plan's own concurrent execute to publish its journal; a re-read of the plan under admission; owner-absent recovery over the target's non-terminal journals; re-verification of every attachment from disk; and the linked insert. **Expiry is checked at admission only**: a plan that expires while the request is on the wire does not invalidate the request, because the message is already gone.

**Uploads.** An inbox attachment goes through the §11 upload transport: `POST /users/self/files` with `parent_folder_path` set to `conversation attachments` and `on_duplicate: "rename"`, then the returned upload URL, with the streamed SHA-256 verified against the frozen digest. An upload failure is `refused`, not `failed`: nothing reached the conversation endpoint, so the journal records `never_sent` and names the attachment that stopped it.

**Events.** Each transition writes an `operation.state` event in the same transaction as the row update, with `dataset: "operation_journal"`, `entity_key` the journal id, `before`/`after` carrying `{ "state": … }`, and the scope `topic:<tid>`, `conversation:<id>`, or `conversation:new`. The dedupe key is `operation:<journal_id>:<state>`, so a replayed execute never writes a second event for a state the journal already reached.

**Cache epochs.** A success bumps, in the same transaction:

| Kind | Scopes |
|---|---|
| `discussion_reply` | `discussion:topic:<tid>`, `discussion:topic:<tid>:replies`, `discussions:course:<cid>` |
| `inbox_send` | `inbox:*`, `inbox_unread:*` |
| `inbox_reply` | `conversation:conversation:<id>`, `inbox:*`, `inbox_unread:*` |

### 25.5 Recovery and ambiguous outcomes

When the owner lock is free but the journal is not terminal, the row is given the state its interruption implies **before** anything is concluded from a readback:

| State found | Recovered to | Why |
|---|---|---|
| `planned` | `refused`, `not_posted_evidence: "never_sent"` | the process died before the request was built; nothing was on the wire |
| `posting` | `outcome_unknown`, `response_kind: "none"` | the request may have been on the wire; the answer is not known |
| any terminal state | unchanged | there is nothing to recover |

This runs over every non-terminal journal of a target before a new one is admitted, so an abandoned operation never blocks a target forever and never turns into a second message. Only `execute` and `operation reconcile` recover; a readback is not a recoverer.

**Nothing is ever resent automatically.** A journal reaches `outcome_unknown` from a timeout, a transport failure after the request was written, a crash during `posting`, or a **5xx answer**. A `5xx` is `outcome_unknown` and not `failed`, because §12.2 records that Canvas can answer `500` after it has committed, so the answer proves nothing and calling it failed would invite a resend. A `4xx` stays `failed`: Canvas rejected the request.

Only `operation reconcile` moves a journal out of `outcome_unknown`, and only on evidence. `--assume-not-posted` records that nothing was posted. It is refused while a matching message is visible in the thread, while the journal is younger than **30 minutes**, and while the readback did not cover the thread — a readback that is not `complete` cannot prove absence, so `not_found` then carries a warning and the assertion is refused however old the journal is. The refusal is a warning on an exit-9 envelope, not a silent success. When it is recorded, the answer states the residual risk §12.2 requires: the original request can still land, and Canvas can store a body whose digest no longer matches what was sent, so writing again may leave two messages.

**A live owner stops recovery, not reading.** When another process holds the owner lock, `status` and `reconcile` still read the thread and report `verdict: "not_read"` with a warning. `operation status` never changes journal state; `operation reconcile` may. Both record what they saw, because recording an observation is not a state change.

### 25.6 Attribution, and what may be claimed

Attribution is **evidence, not confidence**. It is per operation, and it is the only thing a caller may repeat.

| `attribution` | State | What is true |
|---|---|---|
| `accepted` | `posted` | Canvas answered 2xx and named the object it created |
| `observed` | `posted` | a later readback shows that same object id |
| `unproven` | `matched` | the thread holds a message with the same `sent_sha256`, and nothing links it to this request |
| `none` | any other | nothing links this journal to an object in Canvas |

A digest match is `unproven` and never `observed`: two people can write the same sentence, and one person can write it twice. The candidate must also be this identity's own writing — an object Canvas attributes to somebody else resolves nothing, and an object with no author at all leaves the question open.

`delivery` is a separate field, and it is about what Canvas can report at all: `observable` for a discussion reply, `not_observable` for both inbox writes. **A conversation Canvas accepted is not delivered mail.** The human line says "accepted by Canvas". Nothing in either output mode prints "delivered", "received", or "has read", and a test asserts it.

**The response record.** Only an allowlist of the Canvas answer is stored, and the body never is: `id`, `conversation_id`, `created_at`, `created_at_local`, `user_id`, `body_sha256` (of the body Canvas echoed), `attachment_ids`, and `response_sha256` (of the raw HTTP response). A readback stores the same fields plus `scanned` and `complete`.

### 25.7 Exits

These extend §14. No new exit code is added.

| Exit | When |
|---|---|
| 0 | `posted` or `matched` |
| 6 | an unknown journal id, a cross-origin URL, an unparseable operand |
| 8 | every §25.3 refusal, an invalidated plan, a spent handle, and a `4xx` from Canvas (`failed`) |
| 9 | `planned`, `posting`, or `outcome_unknown` — the outcome is unknown, not failed |
| 11 | a declined or cancelled approval |

Exit 9 carries `outcome: "recovery"`. It means "ask again", never "it failed".

### 25.8 Receipts and the pending hook

`receipts list|show|export|acknowledge` cover operation journals beside submissions, in the same `Journal` shape, and the `kind` column is what tells them apart. On an operation row `assignment_id`, `assignment_name`, `baseline_attempt`, `posted`, `readback`, and `server_match` are `null`, the new `operation` block carries the write, and `superseded` is always `false`, because a reply or a message is never superseded (§19 item 40). The course filter still applies, and a journal with no course is kept only when no course was asked for. `receipts show` and `receipts export` accept a journal id or a receipt id, and both id spaces are searched before either is reported missing. `receipt@1` gains the same nullable `operation` block, carrying the kind, the target, the recipients and subject, the digests, the allowlisted response, the readback, the server match, the attribution, and the delivery field.

The §10 **pending hook** covers operation journals. A journal is pending while it is `planned` or `posting`, and an `outcome_unknown` one is pending until it is acknowledged; there is no superseding rule. `discussion@1`, `inbox@1`, `conversation@1`, and `inbox_unread@1` gain `pending: bool` and `pending_journals: [id]`. While `pending` is true the read is not a settled picture of the thread. A send belongs to no conversation until Canvas names one, so the `inbox` and `inbox_unread` reads report it and `inbox show` cannot; the pending query for a conversation therefore also matches any unresolved `inbox_send`.

### 25.9 Schemas

`operation@1` is the result of all three writes and of `operation status`. `operation_reconcile@1` is the result of `operation reconcile`. Both are registered with fixtures (Appendix D). `plan@1` gains a nullable `operation` block and makes `course_id` and `assignment_id` nullable, because an inbox write has neither. `Journal` and `receipt@1` gain the same block (§25.8). Every addition is additive, so each schema keeps `@1`.

`operation@1` names its own plan, so `plan_id` is never `null` there: the unique index on `plan_id` makes the link one-to-one, and the registry's nullability test carries an explicit exception for the two schemas that name their own plan.

### 25.10 The agent surface and the skill

Eight tools join the §21.2 catalog, each calling the same core the CLI calls:

| Tool | Effect | Command behind it |
|---|---|---|
| `discussion.reply.prepare` / `.execute` | Organize / RemoteWrite | `discussion reply` |
| `inbox.send.prepare` / `.execute` | Organize / RemoteWrite | `inbox send` |
| `inbox.reply.prepare` / `.execute` | Organize / RemoteWrite | `inbox reply` |
| `operation.status` | Read | `operation status` |
| `operation.reconcile` | Retire | `operation reconcile` |

A `*.prepare` freezes a plan and posts nothing. `operation.reconcile` is a retirement because `--assume-not-posted` records a durable local decision about what did not happen. `operation.status` keeps `readOnlyHint: true` while it records a readback and can move `attribution` from `accepted` to `observed`; §19 item 38 owns that reading.

An execute on a prepared plan returns `input_required` with a `requestState` and one `elicitation/create` request — the same round trip `submission.execute` uses (§21.2). A host that declares no elicitation support gets a domain refusal instead: exit 8, `details.reason: "approval_required"`, with the handle, and **nothing is dispatched**. An execute on an already-executed plan returns that journal's `operation@1` with `replayed: true` and creates no second message. **There is no argument anywhere that asserts an approval.**

The retry guard asks the catalog which tools ask for approval: any `*.execute` in the catalog may carry a `requestState`, and a replayed state aimed at a read is still rejected as an argument error.

`skill/canvas-cli/reply-and-message-with-approval.md` is the sixth workflow. It carries the REPORT §3.5 course-policy boundary in plain words: **an approval to post is not permission for AI-generated academic work**, and the rule is the course's, not the tool's. It also says never write a placeholder, never invent a recipient, show the exact text and every attachment before the approval, and never turn an acceptance into delivered mail.

## Appendix A. Dependencies (verified on crates.io, 2026-09-09)

| Crate | Version | Role |
|---|---|---|
| clap (derive, env) | 4.6.6 | CLI |
| clap_complete / clap_mangen | 4.6.9 / 0.3.3 | completions, man pages |
| tokio (workspace: rt, macros, fs, time, sync; per crate: io-util, net, signal, rt-multi-thread, process) | 1.53.1 | runtime. **`net` is a production feature of `canvas-cli` since M7-a**: the broker's Unix socket and named pipe need it (§24). `canvas-api` still enables it in dev-dependencies only, for its own test server |
| reqwest (rustls, json, stream, gzip, brotli, multipart) | 0.13.5 | HTTP |
| futures-util | 0.3.34 | streams |
| serde / serde_json | 1.0.229 / 1.0.151 | models, JSON |
| jiff (serde) | 0.2.35 | time |
| keyring (`=4.2.0`, no default features, `v1`) | 4.2.0 | credential store; MSRV 1.88 |
| etcetera | 0.11.0 | XDG paths |
| figment (toml, env) + toml | 0.10.19 / 1.1.5 | config |
| rusqlite (bundled) | 0.40.2 | DBs |
| cap-std + cap-fs-ext (`std`) | 3.4.6 | containment, no-follow opens |
| fs4 | 0.13.1 | file locks (`flock` / `LockFileEx`) |
| comfy-table | 8.0.0 | tables |
| anstyle / anstream | 1.0.14 / 1.0.0 | color |
| indicatif | 0.18.6 | progress |
| open | 5.4.3 | browser |
| sha2 | 0.10.9 | hashes |
| htmd (+ markup5ever_rcdom) | 0.5.5 / 0.38.0 | HTML → Markdown; M1-c picked `htmd` over `html2text` |
| tracing / tracing-subscriber | 0.1.44 / 0.3.23 | logs |
| uuid (serde, v4) | 1.18.1 | journal ids, plan ids, approval handles, identity generations |
| thiserror / anyhow | 2.0.20 / 1.0.104 | errors |
| rmcp (`=3.2.0`, no default features; `server`, `client`, `macros`, `elicitation`, `transport-io`, `transport-async-rw`, `schemars`, `local`) | 3.2.0 | `canvas mcp` (M6-b). `local` is load-bearing: the command cores are `!Send`, so the service runs in a `LocalSet`. Brings `chrono` transitively, see §19 item 18 |
| schemars (`=1.2.2`) | 1.2.2 | JSON Schema for `canvas schema` and the MCP tool schemas (M6-b) |
| dev: wiremock, assert_cmd, predicates, insta | 0.6.5, 2.2.2, 3.1.4, 1.48.0 | tests |
| tools: cargo-nextest, cargo-deny, cargo-dist, release-plz | 0.9.143, 0.20.2, 0.32.0, 0.3.164 | CI, release |

Toolchain: stable 1.98.x in CI; MSRV 1.88; owner machine 1.97.1.

Every version is pinned with `=`, in the workspace manifest or in the crate that uses it. Direct dependencies the table still omits: `getrandom` 0.4.3, `unicode-normalization` 0.1.25, `httpdate` 1.0.3, `rpassword` 7.4.0, `rustix` 1.1.4 (`fs`, `process`; `canvas-cli` under `cfg(unix)` only, for the socket and pipe permission work of §24), and, for tests and `xtask` only, `tokio-rustls` 0.26.5, `tempfile` 3.23.0, and `url` 2.5.8. **No post-v1 package added a dependency after `rmcp` and `schemars`**, M7-a, M7-b, and M8-b included: the companion ships as plain JavaScript with no npm dependency at all, and M7-a's only manifest change was the tokio `net` feature above. `uuid` names journal ids, plan ids, approval handles, operation journal ids, and identity generations.

Re-verified against `Cargo.lock` for this pass: every version in the table matches the lock file exactly. Two crates appear twice in the lock and only one of each is a direct dependency — `toml` (1.1.5 direct, 0.8.23 transitive) and `sha2` (0.10.9 direct, 0.11.0 transitive) — and `getrandom` appears three times for the same reason.

## Appendix B. Canvas endpoints used

| Command | Endpoint |
|---|---|
| auth, doctor | `GET /api/v1/users/self` |
| courses | `GET /api/v1/courses?enrollment_type=student&enrollment_state=active&include[]=term&include[]=total_scores&include[]=current_grading_period_scores&include[]=favorites&per_page=100`; `--all` repeats with `enrollment_state=completed` and `invited_or_pending` |
| course | `GET /api/v1/courses/:id?include[]=term&include[]=syllabus_body&include[]=teachers&include[]=total_scores&include[]=current_grading_period_scores` |
| todo, calendar | `GET /api/v1/planner/items?start_date=…&end_date=…&per_page=100` |
| todo | `GET /api/v1/users/self/missing_submissions?include[]=planner_overrides&include[]=course&per_page=100` |
| assignments, todo `--all` | `GET /api/v1/courses/:id/assignments?include[]=submission&per_page=100` |
| assignment, submit preflight | `GET /api/v1/courses/:id/assignments/:aid?include[]=submission&include[]=can_submit` |
| assignment (graded), submission, verify, reconcile | `GET …/assignments/:aid/submissions/self?include[]=submission_history&include[]=submission_comments&include[]=rubric_assessment` |
| submit | `POST …/assignments/:aid/submissions/self/files`; multipart `POST <upload_url>`; `GET <same-origin Location>`; `POST …/assignments/:aid/submissions` |
| grades | `GET /api/v1/users/self/enrollments?type[]=StudentEnrollment&state[]=active&state[]=completed[&grading_period_id=ID]&per_page=100`; `GET /api/v1/courses/:id/assignment_groups?include[]=assignments&include[]=submission&override_assignment_dates=true[&grading_period_id=ID]&per_page=100`; `GET /api/v1/courses/:id/grading_periods?per_page=100` (wrapped) |
| files, download, verify | `GET /api/v1/courses/:id/folders?per_page=100`; `GET /api/v1/courses/:id/files?per_page=100`; `GET /api/v1/files/:id` |
| modules, download | `GET /api/v1/courses/:id/modules?include[]=items&include[]=content_details&per_page=100`; `GET /api/v1/courses/:id/modules/:mid/items?include[]=content_details&per_page=100` |
| announcements | `GET /api/v1/announcements?context_codes[]=course_N…(≤10)&start_date=…&end_date=…&per_page=100`; `GET /api/v1/courses/:cid/discussion_topics/:id` |
| calendar | `GET /api/v1/calendar_events?type=event&context_codes[]=…(≤10)&start_date=…&end_date=…&per_page=100` |

Added after v1. Every one is a `GET`. `per_page=100` is appended by the client to every paginated collection request whose path does not already carry it; a single-object `GET` never gets it (§11).

| Command | Endpoint |
|---|---|
| pages | `GET /api/v1/courses/:id/pages?sort=title&per_page=100` |
| page | `GET /api/v1/courses/:id/pages/:url_or_id` |
| syllabus | none of its own; reads the `course` dataset (§23) |
| discussions | `GET /api/v1/courses/:id/discussion_topics?only_announcements=false&per_page=100` |
| discussion | `GET /api/v1/courses/:cid/discussion_topics/:tid`; with `--replies` also `GET /api/v1/courses/:cid/discussion_topics/:tid/entries?per_page=100` and `GET /api/v1/courses/:cid/discussion_topics/:tid/entries/:eid/replies?per_page=100` |
| inbox | `GET /api/v1/conversations?scope=inbox\|unread\|sent\|archived&auto_mark_as_read=false&per_page=100` |
| inbox show | `GET /api/v1/conversations/:id?auto_mark_as_read=false` |
| inbox unread-count | `GET /api/v1/conversations/unread_count` |
| submit, plan execute | no new endpoint; the pre-flight read `GET …/assignments/:aid?include[]=submission&include[]=can_submit` runs twice, once to freeze the plan and once to revalidate it (§20) |
| watch, mcp | no new endpoint; both refresh the §10 datasets above |
| here | no new endpoint; the API half calls the `course`, `assignment`, and `announcement` cores above (§24) |

The materialized discussion endpoint `GET …/discussion_topics/:tid/view` is deliberately not used: it marks entries read as a side effect of reading them (§23).

The three writes (§25). Every `GET` here runs at prepare or in a readback; the `POST` runs only after a recorded approval.

| Command | Endpoint |
|---|---|
| discussion reply — prepare | `GET /api/v1/courses/:cid/discussion_topics/:tid`; with `--to`, `GET /api/v1/courses/:cid/discussion_topics/:tid/entries?per_page=100` |
| discussion reply — execute | the prepare read again to revalidate, then `POST /api/v1/courses/:cid/discussion_topics/:tid/entries` with `{ message }`, or `POST …/entries/:eid/replies` with `--to` |
| inbox send — prepare | `GET /api/v1/search/recipients?user_id=<id>`, once per recipient |
| inbox send — execute | `POST /api/v1/conversations` with `{ recipients, subject, body, group_conversation: false, attachment_ids }` |
| inbox reply — prepare, and execute's revalidation | `GET /api/v1/conversations/:id?auto_mark_as_read=false` |
| inbox reply — execute | `POST /api/v1/conversations/:id/add_message` with `{ body, attachment_ids }` |
| an inbox attachment | `POST /api/v1/users/self/files` with `parent_folder_path=conversation attachments` and `on_duplicate=rename`, then the returned upload URL (§11) |
| operation status, operation reconcile | `GET /api/v1/courses/:cid/discussion_topics/:tid/entries?per_page=100`, or `GET …/entries/:eid/replies?per_page=100` for a threaded reply, or `GET /api/v1/conversations/:id?auto_mark_as_read=false` |

`group_conversation` is sent as `false`, always: group writes are out of this package.

The companion (§24) makes exactly one Canvas request, and it does not make it from this machine's HTTP client at all:

| Caller | Endpoint |
|---|---|
| the extension's content script, in the attached tab | `GET <origin>/api/v1/users/self`, same-origin, with the browser's own cookies, `redirect: "error"`, and the body reduced to `{ user_id, observed_at }` before it leaves the page |

No token of this CLI is ever sent through the browser, and no Canvas request is ever proxied through it.

## Appendix C. What the research and the reviews changed

- Local grade math, target solver, GraphQL, ETags, hard links, group submissions: out of v1.
- Identity = (canonical origin, user id) with a lossless key; profiles are labels; aliases are identity-owned.
- Transactional journal in state.sqlite with admission and owner locks per operation and owner-absent recovery; receipts materialized in the success transaction and bound to the posted attempt with evidence provenance; allowlisted response records; reconcile never claims authorship: file journals become `matched` (attribution unproven), text/URL journals stay unknown with a server match; no `POST` status is taken as proof of rollback: every non-success is unknown until history is read; there is no automatic negative inference for a dispatched `POST`; a journal stays unknown until positive history evidence appears or the user explicitly assumes a negative outcome after 30 minutes.
- cap-std + cap-fs-ext no-follow traversal with retained descriptors, install mutex, manifest with move protocol, ownership/clobber table, size-checked installs.
- Mutation epochs in state.sqlite; scoped entity values with composite observation keys and per-field freshness in the cache.
- keyring 4.2.0 `v1` feature route; MSRV 1.88.
- Complete JSON schemas (Appendix D); single-envelope and exit-precedence rules.
- Dependency-ordered packages with per-round shared-file owners.

### What the post-v1 rounds changed

`docs/agent-ux/REPORT.md` proposed an agent-first surface on top of the v1 contract. These packages are on `main`; each has its own code review, and §§20–23 record what they built rather than what was proposed.

| Package | What it added | Section | Review |
|---|---|---|---|
| M6-a | operation plans, approval handles, the approval audit, the human `submit` refactor | §20 | `docs/reviews/code-M6-a.md` |
| M6-b | `canvas schema`, `canvas mcp`, the shipped skill, the host matrix | §21 | `docs/reviews/code-M6-b.md` |
| M6-c | cross-process permits, the shared governor, refresh single-flight, foreground interest, the event log, `canvas watch`, `canvas notify` | §22 | `docs/reviews/code-M6-c.md` |
| M6-c2 | `subscriptions/listen` over the event log | §22.5 | `docs/reviews/code-M6-c2.md` |
| M8-a | `pages`, `page`, `syllabus`, `discussions`, `discussion`, `inbox *`, the rubric extension | §23 | `docs/reviews/code-M8-a.md` |
| M8-a2 | the eight M8-a read tools on the agent surface; §19 items 27 and 28 applied | §21, §23 | `docs/reviews/code-M8-a2.md` |
| M8-a3 | the `inbox.unread_count` event; the per-form `canvas schema` pages | §22, §21 | `docs/reviews/code-M8-a3.md` |
| M7-a | the Chrome companion, the native host, the broker ownership lock, `bridge-native@1`, `bridge-ipc@1`, zones, the account probe, `canvas bridge`, `canvas here`, the `context.*` tools and the `/context` resource | §24 | `docs/reviews/code-M7-a.md` |
| M7-b | the side panel, notes, follow, panel approvals, and the plan-decision events | §24 | `docs/reviews/code-M7-b.md` |
| M8-b | the three plan kinds, the operation journal, `operation status\|reconcile`, the attribution ladder, and the sixth skill workflow | §25 | `docs/reviews/code-M8-b.md` |

Every post-v1 package REPORT §4 scheduled is now on `main`. M8-c (GraphQL) and M8-d (OAuth) stay conditional, as REPORT §4 leaves them.

`docs/reads-v2.md` was the M8-a contract document, `docs/companion.md` the M7-a and M7-b one, and `docs/writes-v2.md` the M8-b one. Their content is now §23, §24, and §25, and each file is a pointer. `docs/companion.md` keeps one thing of its own that no section replaces: the table of Chrome checks a person runs by hand, because nothing in that package has been run in a real browser (§24.16).

## Appendix D. JSON `result` payloads

Types: `id` = string; `ts` = RFC 3339 UTC; `ts+local` = also `<name>_local`; `date` = civil `YYYY-MM-DD`; `T?` = nullable; arrays are never `null`. Every listed field is always present.

**Shared objects**

- `Course` = `{ id, code, name, term { id?, name?, start_at?, end_at? }, enrollment_state, is_favorite: bool, restricted: bool, html_url }`
- `Posted` = `{ evidence: "post-response"|"history-files", submission_id?, attempt: number, submitted_at?: ts+local, workflow_state?: string, late?: bool, missing?: bool, excused?: bool, submission_type?: string, attachments: [Attachment], body_sha256?: string, url?: string, response_sha256?: string }`
- `Readback` = `{ submitted_at?: ts+local, late?: bool, attachments: [Attachment], body_sha256?: string }`
- `Candidate` = `{ attempt: number, submitted_at?: ts+local, attachment_ids: [id] }` (`attachment_ids = []` for text and URL entries)
- `Journal` = `{ journal_id, state, owner: "live"|"absent"|"n/a", superseded: bool, acknowledged_at?: ts, plan_id?: string, approval?: Approval, course_id?, course_code?, assignment_id?, assignment_name?, kind, baseline_attempt?: number, created_at: ts, updated_at: ts, uploaded_file_ids: [id], post_status?: number, response_kind?: "canvas-error"|"other"|"none", not_submitted_evidence?: "never_sent"|"assumed", posted?: Posted, readback?: Readback, server_match?: Candidate, receipt_id?, error?: string, operation?: Operation }` — one shape for both journal kinds, and `kind` says which. On an operation journal `assignment_id`, `assignment_name`, `baseline_attempt`, `posted`, `readback`, and `server_match` are `null`, `operation` carries the write, `course_id` is `null` for an inbox write, and `superseded` is always `false` (§25.8, §19 item 40)
- `Operation` = `{ kind: "discussion_reply"|"inbox_send"|"inbox_reply", course_id?, topic_id?, parent_entry_id?, conversation_id?, recipients: [id], subject?: string, state, input_sha256, transform, sent_sha256, server_body_sha256?: string, attachments: [OperationAttachment], posted?: OperationResponse, readback?: OperationReadback, server_match?: OperationMatch, attribution: "accepted"|"observed"|"unproven"|"none", delivery: "observable"|"not_observable" }` — the operation block on `Journal` and `receipt@1` (§25)
- `OperationTarget` = `{ kind, course_id?, course_code?, topic_id?, topic_title?: string, parent_entry_id?, conversation_id?, conversation_subject?: string, recipients: [id], recipient_names: [string] }`
- `OperationAttachment` = `{ name, size: number, sha256, canvas_file_id? }`
- `OperationResponse` = `{ id?, conversation_id?, created_at?: ts+local, user_id?, body_sha256?: string, attachment_ids: [id], response_sha256?: string }` — the allowlisted record of Canvas' answer; the body is never stored (§25.6)
- `OperationReadback` = `OperationResponse` without `conversation_id` and `response_sha256`, plus `read_at: ts`, `scanned: number`, and `complete: bool`
- `OperationMatch` = `{ id, created_at?: ts+local, user_id?, body_sha256 }` — a candidate matched by digest alone, with no id link
- `Approval` = `{ channel: "tty"|"elicitation"|"panel"|"yes-flag", at: ts, consumer?: string, plan_sha256 }` — the recorded human approval of the plan that produced the journal (REPORT §3.5); `plan_id` and `approval` are `null` for journals created before plans existed (M6-a).
- `Grade` = `{ current_score?: number, current_grade?: string, final_score?: number, final_grade?: string, period { mode: "all"|"current"|"id", id?: id, title?: string } }`
- `SubmissionStatus` = `{ submitted?: bool, graded?: bool, score?: number, grade?: string, late?: bool, missing: bool, excused?: bool, workflow_state?: string, submitted_at?: ts+local, attempt?: number, posted_at?: ts, pending: bool }`
- `Availability` = `{ locked?: bool, lock_explanation?: string, submittable?: bool, external?: bool, unlock_at?: ts+local, lock_at?: ts+local }`
- `Attachment` = `{ id, display_name, size?: number, content_type?: string }`
- `Freshness` = envelope entry `{ dataset, scope, source: "cache"|"network", fetched_at?: ts, complete: bool, count?: number, stale: bool }`
- `Listing` = `{ available: bool, http_status?: number }` — the shape `files@1` already used, reused by `pages@1`, `discussions@1`, and `inbox@1` (§23)
- `Embedded` = `{ kind: "iframe"|"lti"|"video"|"audio"|"unknown", src_origin?: string, reported: "unavailable" }`
- `FileRef` = `{ file_id, name?: string, url }`; `ExternalLink` = `{ url }` — both stripped of every capability-bearing part (§23)
- `Participant` = `{ id?, name?: string }`
- `Note` = `{ note_id, consumer, text, source_refs: [string], at: ts, generation: number }` — one inert note held for display (§24.11)
- `Follow` = `{ request_id, url, dispatched: bool, dispatched_at: ts, dispatch_ms: number, generation: number, load: "loaded"|"unknown", load_at?: ts }` — a dispatch acknowledgement and, later, what became of it. `load` is never `failed` (§24.12)

| Schema | `result` | Sort |
|---|---|---|
| `courses@1` | `{ courses: [ Course & { grades: Grade } ] }` | `code` asc, then `id` asc |
| `course@1` | `{ course: Course & { grades: Grade, teachers: [ { id, name } ], syllabus_markdown?: string, time_zone?: string, modules_count?: number } }` | — |
| `todo@1` | `{ window { start: date, end: date, days: number }, items: [ { key, kind, raw_type: string, id, assignment_id?, parent_assignment_id?, course_id?, course_code?, title, due_at?: ts+local, scheduled_at?: ts+local, points_possible?: number, status: SubmissionStatus, availability: Availability, marked_complete: bool, dismissed: bool, html_url?: string } ], counts { missing, due_today, due_week, hidden } }` | `(scheduled_at ?? due_at)` asc, undated last, then `course_code`, then `id` |
| `assignments@1` | `{ course_id, bucket, assignments: [ { id, course_id, name, due_at?: ts+local, points_possible?: number, submission_types: [string], allowed_extensions: [string], allowed_attempts?: number, group_assignment: bool, availability: Availability, status: SubmissionStatus, html_url } ] }` | `due_at` asc, undated last, then `id` |
| `assignment@1` | `{ assignment: <assignments item> & { description_markdown?: string, can_submit?: bool, extra_attempts?: number, rubric: [ { id, description, points?: number } ], rubric_assessed: bool, rubric_assessment: [ { criterion_id, points?: number, comments?: string } ], comments_count?: number, external_tool_name?: string } }` | — |
| `submit@1` | `{ outcome, state, journal_id, receipt_id?, attribution?: "observed"|"unproven", post_status?: number, response_kind?: "canvas-error"|"other"|"none", posted?: Posted, server_match?: Candidate, candidates: [Candidate], files: [ { name, size: number, sha256, canvas_file_id?: id } ], text?: { input_sha256, transform, sent_sha256 }, url?: string, error?: string }` | candidates by `attempt` asc |
| `submission@1` | `{ submission: SubmissionStatus & { submission_type?: string, body_sha256?: string, url?: string, attachments: [Attachment], comments: [ { id, author?: string, created_at: ts+local, text } ], rubric_assessed: bool, rubric_assessment: [...] }, history: [ { attempt: number, submitted_at?: ts+local, score?: number, attachments: [Attachment] } ], pending_journals: [id] }` | history by `attempt` asc |
| `receipt@1` | `{ receipt_id, journal_id, identity { origin, user_id, key }, course_id, course_code?, assignment_id, assignment_name?, kind, baseline_attempt: number, attribution: "observed"|"unproven", posted: Posted, readback?: Readback, files: [ { name, size: number, sha256, canvas_file_id?: id } ], text?: { input_sha256, transform, sent_sha256, server_body_sha256?: string }, url?: string, due_at?: ts, plan_id?: string, approval?: Approval, operation?: Operation, cli_version, created_at: ts }` (export file = the document; `receipts show --json` = envelope with `result.receipt`) | — |
| `receipts@1` | `list`: `{ journals: [Journal] }`; `show`: `{ journal: Journal, receipt?: <receipt@1 document> }`; `export`: `{ receipt_id, path?: string, bytes: number }` (`--out -` is raw output, §7); `acknowledge`: `{ journal_id, acknowledged_at: ts }` | `created_at` desc, then `journal_id` |
| `verify@1` | `{ outcome: "verified"|"verified_body"|"mismatch"|"unavailable"|"refused", receipt_id, attempt?: number, attribution?: "observed"|"unproven", files: [ { canvas_file_id, name?: string, expected_sha256?: string, actual_sha256?: string, status: "ok"|"mismatch"|"unavailable"|"missing"|"extra" } ], body?: { expected_sha256?: string, actual_sha256?: string, status: "ok"|"mismatch"|"unavailable" }, reason?: string }` (`expected_sha256` is `null` for `extra`, `actual_sha256` is `null` for `missing`/`unavailable`) | by `canvas_file_id` |
| `reconcile@1` | `{ outcome: "ok"|"recovery"|"refused", state, journal_id, owner: "live"|"absent"|"n/a", response_kind?: "canvas-error"|"other"|"none", not_submitted_evidence?: "never_sent"|"assumed", assume_available: bool, attribution?: "observed"|"unproven", receipt_id?, posted?: Posted, server_match?: Candidate, candidates: [Candidate], message: string }` | candidates by `attempt` asc |
| `grades@1` | `{ period_mode, courses: [ { course: Course, grades: Grade, unavailable_reason?: string } ], course?: { groups: [ { id, name, position: number, weight?: number, rules { drop_lowest?: number, drop_highest?: number, never_drop: [id] }, subtotal?: { score?: number, possible?: number }, assignments: [ { id, name, points_possible?: number, omit_from_final_grade: bool, status: SubmissionStatus } ] } ], periods: [ { id, title, start_date?: ts, end_date?: ts, is_current: bool } ] } }` | courses by `code`; groups by `position`; assignments by `due_at` then `id` |
| `files@1` | `{ course_id, listing: { available: bool, http_status?: number }, files: [ { id, source: "listing"|"module", folder_id?, folder_path?: string, module_id?, module_position?: number, name, size?: number, updated_at?: ts, hidden?: bool, locked?: bool, lock_explanation?: string } ] }` | `folder_path`/module path asc, then `name`, then `id` |
| `modules@1` | `{ course_id, modules: [ { id, name, position: number, state?: string, items_count?: number, items_complete: bool, items: [ { id, type, content_id?, title, position: number, locked?: bool, lock_explanation?: string, completed?: bool, html_url?: string } ] } ] }` | `position` |
| `download@1` | `{ dest, dry_run: bool, courses: [ { course_id, course_code, files: [ { id, path, previous_path?: string, action: "planned"|"downloaded"|"moved"|"skipped"|"unmanaged"|"modified"|"locked"|"unavailable"|"skipped_external"|"unsafe_path"|"unresolved_move"|"failed", size?: number, error?: string, verify?: "ok"|"mismatch" } ] } ], totals { planned, downloaded, moved, skipped, unmanaged, modified, locked, unavailable, skipped_external, unsafe_path, unresolved_move, failed, bytes } }` | `path` asc |
| `announcements@1` | `{ window { start: date, end: date }, announcements: [ { id, course_id, course_code?, title, posted_at: ts+local, author?: string, read: bool, html_url } ] }` | `posted_at` desc, then `id` |
| `announcement@1` | `{ announcement: <announcements item> & { message_markdown?: string } }` | — |
| `calendar@1` | `{ window { start: date, end: date }, items: [ { uid, kind, id, course_id?, course_code?, title, is_deadline: bool, due_at?: ts+local, start_at?: ts+local, end_at?: ts+local, all_day: bool, all_day_date?: date, html_url?: string } ] }` | `(all_day_date ?? start_at ?? due_at)` asc, then `uid` |
| `open@1` | `{ target_kind, id, url, launched: bool }` | — |
| `sync@1` | `{ datasets: [ Freshness & { requests: number, error?: string } ] }` | dataset, scope |
| `cache@1` | `stats`: `{ path, size_bytes, tables: [ { name, rows } ], datasets: [Freshness] }`; `clear`: `{ cleared: bool, rows_deleted: number }`; `path`: `{ path }` | — |
| `alias@1` | `{ aliases: [ { name, course_id, course_code? } ] }` | `name` |
| `auth_status@1` | `{ profile?, identity?, token_source?: "env"|"keyring"|"file", stray_sources: [string], backend?: string, validated_at?: ts }` | — |
| `auth_login@1`, `auth_logout@1` | `{ profile, identity, backend?, removed: [string] }` (`removed` for logout) | — |
| `identity@1` | `{ identities: [ { key, origin, user_id, name?, profiles: [string], size_bytes: number, journals_pending: number } ] }`; `remove`: `{ removed: bool, key, profiles_removed: [string], default_profile_cleared: bool }` | `key` |
| `config@1` | `get`: `{ key, value }`; `set`: `{ key, value, previous? }`; `path`: `{ path }` | — |
| `doctor@1` | `{ identity_selected: bool, checks: [ { name, status: "ok"|"warn"|"fail"|"skipped", message } ], recovered_journals: [id] }` | fixed order |
| `version@1` | `{ version, commit?, target }` | — |
| `plan@1` | `{ plan: { plan_id, state: "prepared"\|"approved"\|"executed"\|"expired"\|"invalidated", consumer?: string, course_id?, course_code?, assignment_id?, assignment_name?, kind: "online_upload"\|"online_text_entry"\|"online_html"\|"online_url"\|"discussion_reply"\|"inbox_send"\|"inbox_reply", baseline_attempt: number, estimated_attempt: number, files: [ { name, size: number, sha256 } ], text?: { input_sha256, transform, sent_sha256 }, url?: string, comment_chars?: number, due_at?: ts, plan_sha256, created_at: ts, expires_at: ts, approval?: Approval, journal_id?, invalidated_reason?: string, operation?: { kind, target: OperationTarget, subject?: string, text: { input_sha256, transform, sent_sha256 }, attachments: [OperationAttachment] } } }` (§20, §25). `course_id` and `assignment_id` are `null` on an operation plan, which names no assignment and, for an inbox write, no course; `operation` is `null` on a submission plan, and `kind` says which half to read | — |
| `watch@1` | `{ since?: string, cursor?: string, events: number, ticks: number, resync_required: bool, skipped?: "foreground_interest"\|"journal_in_flight", datasets: [ Freshness & { requests: number, error?: string } ] }` (§22) | dataset, scope |
| `pages@1` | `{ course_id, listing: Listing, pages: [ { id, title?: string, url?: string, updated_at?: ts, published?: bool, front_page?: bool } ] }` | `title` asc, from `sort=title` |
| `page@1` | `{ page: { id, course_id, title?: string, url?: string, updated_at?: ts, published?: bool, front_page?: bool, locked_for_user?: bool, html_url?: string, body_markdown?: string, truncated: bool, embedded: [Embedded], files: [FileRef], external_links: [ExternalLink] } }` | — |
| `syllabus@1` | `{ course_id, syllabus_markdown?: string, truncated: bool, embedded: [Embedded], files: [FileRef], external_links: [ExternalLink], updated_at?: ts }` | — |
| `discussions@1` | `{ course_id, listing: Listing, discussions: [ { id, course_id?, title?: string, posted_at?: ts, last_reply_at?: ts, author?: string, read_state?: string, unread_count?: number, reply_count?: number, locked?: bool, pinned?: bool, is_announcement?: bool, require_initial_post?: bool, assignment_id?, points_possible?: number, group_category_id?, html_url?: string } ] }` | as Canvas returns them |
| `discussion@1` | `{ discussion: <discussions item> & { discussion_type?: string, group_topic_children: [ { id?, group_id? } ], message_markdown?: string, truncated: bool, embedded: [Embedded], files: [FileRef], external_links: [ExternalLink], replies: [ { id, parent_id?, user_id?, user_name?: string, created_at?: ts, message_markdown?: string, truncated: bool, read_state?: string, replies_count: number } ], replies_page: number, replies_total?: number, replies_coverage: { pages_fetched: number, complete: bool, blocked?: "initial_post_required"\|"page_failed"\|"not_requested" }, pending: bool, pending_journals: [id] } }` | replies in the order the reply fetch stored them: every entry page, then the nested replies, then `id` |
| `inbox@1` | `{ scope, listing: Listing, conversations: [ { id, subject?: string, workflow_state?: string, last_message_at?: ts, message_count?: number, context_name?: string, starred?: bool, participants: [Participant] } ], pending: bool, pending_journals: [id] }` | as Canvas returns them |
| `conversation@1` | `{ conversation: { id, subject?: string, workflow_state?: string, last_message_at?: ts, context_name?: string, participants: [Participant], messages: [ { id?, author_id?, created_at?: ts, body?: string, truncated: bool, attachments: [ { file_id?, name?: string, size?: number } ] } ], messages_complete: bool }, pending: bool, pending_journals: [id] }` | messages as Canvas returns them |
| `inbox_unread@1` | `{ unread_count?: number, pending: bool, pending_journals: [id] }` | — |
| `here@1` | `{ attachment?: id, state: "attached"\|"validating"\|"paused"\|"not_attached", consumer?: string, identity { key, generation }, api { course?: <course@1 envelope>, assignment?: <assignment@1 envelope>, announcement?: <announcement@1 envelope> }, browser?: { origin, account { user_id, observed_at: ts }, zone: "open"\|"graded"\|"assessment"\|"external"\|"unknown", page_kind?: string, course_id?, assignment_id?, topic_id?, quiz_id?, page_url?: string, url?: string, title?: string, document_id, frame_id: number, navigation_generation: number, observed_at: ts, ttl_ms: number, selection?: string, text?: string, selection_bytes: number, text_bytes: number, truncated: bool, content_reason?: string, follow?: Follow, notes: [Note] }, reason?: string }` (§24). `api` holds whole §7 envelopes, each with its own freshness; `ttl_ms` is always `0`; `browser` is `null` on a refusal and `reason` names it | notes oldest first |
| `note@1` | `{ attachment?: id, consumer?: string, note?: Note, held: number, reason?: string }` (§24) | — |
| `follow@1` | `{ attachment?: id, consumer?: string, target_kind, id, url, follow?: Follow, side_effects: [string], reason?: string }` (§24). Printed by `open --follow`, so it has no command name of its own | — |
| `bridge@1` | `status`: `{ endpoint, manifest { browser, path?: string, present: bool, host_name, extension_id? }, owner { live: bool, pid?: number, started_at?: ts }, attachments: [ { state, origin, account_user_id, zone, consumers: [string], attached_at: ts, navigation_generation: number } ] }` — the listing names the state, the origin, the account, the zone, and the consumers, and never the attachment id or the page (§24.8); `install`: `{ browser, host_name, extension_id, binary, manifest_path, written: bool, steps: [string] }`; `detach`: `{ detached: bool, attachment_id?: id, reason?: string }` (§24) | — |
| `operation@1` | `{ outcome, kind: "discussion_reply"\|"inbox_send"\|"inbox_reply", state: "planned"\|"posting"\|"posted"\|"matched"\|"outcome_unknown"\|"refused"\|"failed", journal_id, plan_id, replayed: bool, receipt_id?, attribution: "accepted"\|"observed"\|"unproven"\|"none", delivery: "observable"\|"not_observable", post_status?: number, response_kind?: string, not_posted_evidence?: "never_sent"\|"assumed", target: OperationTarget, subject?: string, text { input_sha256, transform, sent_sha256 }, attachments: [OperationAttachment], response?: OperationResponse, readback?: OperationReadback, server_match?: OperationMatch, acknowledged_at?: ts, error?: string }` — the result of all three writes and of `operation status` (§25). `plan_id` is never `null` here | — |
| `operation_reconcile@1` | `{ outcome, journal_id, kind, state, verdict: "observed"\|"matched"\|"not_found"\|"not_read"\|"assumed_not_posted", owner: "live"\|"absent"\|"n/a", attribution, delivery, receipt_id?, readback?: OperationReadback, server_match?: OperationMatch, assume_not_posted_available: bool, message: string }` (§25) | — |
| `error@1` | `{ code, message, http_status?: number, server_errors: [string], details: object }` (exit 8 adds `details.reason`, §14) | — |

**`canvas-cli/event@1` is not a `result`.** It is one self-describing document per line of the `canvas watch --jsonl` stream, with no §7 envelope around it (§22):

```json
{ "schema": "canvas-cli/event@1", "cursor": "12", "kind": "due.changed",
  "observed_at": "2026-09-09T17:05:12Z", "observed_at_local": "2026-09-09T13:05:12-04:00",
  "identity": { "origin": "…", "user_id": "1", "key": "…" },
  "generation": "01234567-89ab-4cde-8f01-23456789abcd",
  "dataset": "assignments", "scope": "course:100", "entity_key": "9",
  "before": { "due_at": "2026-09-10T03:59:00Z" }, "after": { "due_at": "2026-09-12T03:59:00Z" } }
```

`before` and `after` carry the allowlisted fields of the shape only (§22). `entity_key` is `null` when the event is not about one entity.

**Fields added after v1, all additive, so every schema keeps `@1`:**

| Schema | Added | Package |
|---|---|---|
| `Journal`, `receipt@1` | `plan_id?`, `approval?` | M6-a |
| `submit@1` | `replayed: bool` | M6-a, §20 |
| `assignment@1` | rubric criterion gains `long_description?`, `criterion_use_range: bool`, `ratings: [ { id, description?: string, long_description?: string, points?: number } ]`; rubric assessment gains `rating_id?` | M8-a |
| `submission@1` | rubric assessment gains `rating_id?` | M8-a |
| `discussion@1` | `replies_page`, `replies_total?` | M8-a2, §19 item 28 |
| `plan@1` | `operation?`; `course_id` and `assignment_id` become nullable | M8-b, §25 |
| `Journal`, `receipt@1` | `operation?`; on `Journal`, `course_id`, `assignment_id`, `assignment_name`, and `baseline_attempt` become nullable | M8-b, §25.8 |
| `discussion@1`, `inbox@1`, `conversation@1`, `inbox_unread@1` | `pending: bool`, `pending_journals: [id]` | M8-b, §10 pending hook |

## Appendix E. Review response ledger

### Round 1 (v0.1 → v0.2 → v0.3)

| ID | Status after v0.3 |
|---|---|
| B01–B03, M11–M14, N04 | Deferred to v2 with the contract in §12.4. |
| B04 | Lossless identity key, selection matrix, env-pair rules, identity-owned aliases, `--replace` (§8). |
| B05 | Origin rule, manual redirects, authenticated same-origin first download hop, no raw bodies on disk (§11, §12.2). |
| B06 | Transactional journal, single terminal state, attempt binding, reconcile rules (§12.2); recovery reworked to owner locks in v0.4 (R3-B02). |
| B07 | No-follow traversal with retained handles, destination lock, clobber table (§12.3). |
| B08 | keyring 4.2.0 `v1` route, MSRV 1.88 (§8, Appendix A). |
| M01–M03 | Missing always visible, `--missing` precedence, kinds incl. checkpoints/peer reviews/unknown, `scheduled_at`, tri-state `submittable`, nullable `external`, thin/full freshness (§12.1, §10). |
| M04–M06, M09, M16, M25, M27, M30, M34 | Resolved in v0.2; unchanged. |
| M07 | Items route fixed; completeness by absence, `null`, or `items_count` (§10). |
| M08 | Denial classification; `hidden` and `locked` separate fields (§12.3, Appendix D). |
| M10 | Context hash in scope keys; civil all-day dates (§10, §12.5). |
| M13 | Period modes with matching enrollment and group fetches; wrapped periods (§12.4, §10). |
| M15 | Error mapping, no-follow `fstat` read, locked read-modify-replace, logout scope (§8). |
| M17, M18, M19 | Entities vs membership, context-hashed coverage, field groups, epoch abort at commit, pending-journal read hook, `DELETE`+`VACUUM` clear, identity lock (§10; epochs and scoped values reworked in v0.4, R3-M04/M05). |
| M20 | Offline matrix, coverage-based success (§8). |
| M21, M22, M23 | Eligibility rules with unlimited attempts and group refusal; frozen text bytes and transforms; receipt bound to posted attempt; verify outcomes (§12.2). |
| M24, M26 | Destination binding, ownership rule, Windows names, byte-boundary truncation; `expected_size` check, `Accept-Encoding: identity`, upload idle timeout (§12.3, §11). |
| M28, M29, M31 | Appendix D complete; single envelope, `outcome`, precedence, `requests` counters (§7, §14). |
| M32 | Blocking bridge, credential deferral, full-command and cold benchmarks (§13). |
| M33 | Observation ordering, header ageing, admission limit, recovery threshold, initial + 4 retries (§11). |
| M35, M36 | `Secret` in the API crate; dependency edges; per-round owners; network doctor, fixture tooling, `sync --full` assembly assigned (§18). |
| M37 | `201` empty-body branch (§11). |
| N01 | Conditional requests removed from v1. |
| N02 | `all_day_date` carried through (§12.5, Appendix D). |
| N03 | ICS rules plus `--alarm` (§12.5). |
| N05 | Grammar reconciled: `--alarm`, `identity remove`, no bare announcement IDs, default bucket `open`, resolution fallback to all cached courses, UID with identity key (§5, §6, §12.5). |
| N06 | Ledger corrected; the v2 caution is now a note in §12.6. |

### Round 2 (v0.2 → v0.3)

| ID | Resolution |
|---|---|
| R2-B01 | Baseline attempt and frozen intent journaled; receipt fields only from the POST response and the history entry with the same attempt; reconcile candidate rules with uniqueness; no match ≠ failure (§12.2). |
| R2-B02 | `state.sqlite` is the sole authority; JSON files are exports; `BEGIN IMMEDIATE` per transition with expected-state guards; single terminal `submitted`; file-ID reuse promise removed (`--resume` v2) (§12.2); startup recovery replaced by owner locks in v0.4 (R3-B02). |
| R2-B03 | Clobber table: unmanaged and modified files are never replaced without `--force`; ownership proven against the manifest row; exclusive lock during install (§12.3). |
| R2-B04 | Raw bodies hashed in memory only; allowlisted response record; `0600` exports; sanitizer applies the allowlist (§12.2, §15). |
| R2-M01 | Canonical origin with port rules; lossless identity key; `identity.json` verified on open; identity-owned aliases (§8). |
| R2-M02 | Selection matrix incl. explicit profile + env, env pair offline, identity-free commands, `--replace` (§8, §5). |
| R2-M03 | keyring `v1` feature route; error mapping via `store_status`; no Debug of keyring errors; locked read-modify-replace with `O_NOFOLLOW` + `fstat`; logout partial failure (§8). |
| R2-M04, R2-M05 | Period modes with matching sources and cache keys; wrapped paginated grading periods (§12.4, §10). |
| R2-M06 | Course-scoped items route; completeness by absence, `null`, or `items_count`; `items_complete` per module (§10, Appendix D). |
| R2-M07 | Entity/membership split; context hash in window scopes; thin vs full coverage with field precedence (§10). |
| R2-M08 | Pending-journal read hook; invalidation on reconcile; epoch re-check inside the commit transaction, epochs durable in `state.sqlite` since v0.4 (§10). |
| R2-M09 | `DELETE`+`VACUUM` clear; shared/exclusive identity lock (§10). |
| R2-M10 | Offline matrix; coverage-based success incl. `count = 0`; doctor `skipped` (§8, §5). |
| R2-M11, R2-M12 | Kinds table incl. checkpoints, peer reviews, unknown; `scheduled_at`; missing always visible; tri-state `submittable`; nullable `external`; freshness precedence (§12.1). |
| R2-M13 | `open_dir_nofollow` per component with retained handles; no-follow final metadata; reparse points refused (§12.3). |
| R2-M14 | Destination bound to identity; module ownership rule; filter-independent planning; Windows character and device rules; UTF-8 boundary truncation; move on rename (§12.3). |
| R2-M15 | Authenticated same-origin first hop; classification order; `hidden` vs `locked` separate (§11, §12.3). |
| R2-M16 | `expected_size` from file metadata; `Accept-Encoding: identity`; upload connect/idle policy (§11). |
| R2-M17 | Empty-body `201` uses `Location`; missing/off-origin `Location` is `UploadIncomplete` (§11). |
| R2-M18 | Inputs frozen at pre-flight; outbound bytes journaled; transform rules incl. CRLF and empty; `html-verbatim` (§12.2). |
| R2-M19 | Unlimited/missing attempt semantics; `can_submit` governs; group assignments refused (§12.2, §1). |
| R2-M20 | Outcome enum, single envelope, precedence; URL verify refused (exit 8); text verify distinct; receipt validation before fetch (§7, §14, §12.2). |
| R2-M21 | `all_day_date` and exclusive end date through cache, JSON, and ICS (§12.5, Appendix D). |
| R2-M22 | Appendix D rewritten with every command, types, nullability, sorts; receipt shape shared (Appendix D). |
| R2-M23 | `Secret` in `canvas-api`; M0-c depends on M0-b; M1-a acceptance moved; resolver/normalization edges; per-round owners; doctor network, fixtures, `sync --full` assigned (§18). |
| R2-M24 | Blocking I/O bridge with bounded chunks; `fsync` and keyring on blocking workers; full-command, cold, and contention benchmarks (§13). |
| R2-M25 | Initial + 4 retries; observation ordering; header ageing; admission limit without cancellation; recovery threshold; cost as telemetry (§11). |
| R2-N01 | Conditional requests removed from v1 (§1). |
| R2-N02 | Grammar reconciled (§5, §6, §12.5). |
| R2-N03 | Ledger corrected; v2 note in §12.6. |

### Round 3 (v0.3 → v0.4)

| ID | Resolution |
|---|---|
| R3-B01 | Text/URL journals are never attributed: `server_match` is recorded, state stays `outcome_unknown`, no receipt. `posted.evidence` distinguishes `post-response` from history evidence; `response_sha256` is `null` for history. (v0.5: file journals with an exact attachment-set match become `matched` with `attribution = "unproven"`, R4-M01.) (§12.2, Appendix D). |
| R3-B02 | Per-journal exclusive owner lock held across network waits; startup recovery removed; owner-absent recovery only by `submit`, `reconcile`, `doctor` after a non-blocking lock probe, with expected-state guards; reads never transition; `reconcile` eligibility table with idempotent `submitted` (§12.2, §9). |
| R3-M01 | Move is an install branch: requires local match, remote unchanged, target `absent`, all under the install mutex; `pending_move_to` marker with startup resolution; otherwise a new download (§12.3). |
| R3-M02 | `install.lock` is an exclusive install mutex held only for classification → rename → manifest commit; transfers outside it; in-process `tokio::sync::Mutex`; `dest.json` created under the mutex; 30 s timeout → `failed`; final files opened no-follow through the parent handle and the descriptor retained (§12.3). |
| R3-M03 | Identity lock moved to `<data root>/locks/`, never deleted; `identity.json` carries a generation; waiters re-verify after acquiring; removal order credentials → directory → profiles (§9, §10). |
| R3-M04 | Scoped entity keys (`enrollment_grades(enrollment_id, period)`, `course_totals(course_id, mode)`); assignment-group lists per period as memberships; field groups with per-group `observed_at`; absent fields never written (§10). |
| R3-M05 | Hit predicate requires `stale = 0` and `epoch_seen ≥ state epoch`; epochs live in `state.sqlite` and are written in the journal transaction; `cache clear` cannot reset them; failed refresh serves `stale: true` (§10). |
| R3-M06 | Field-group freshness: newest supplier of a field wins; fresh `can_submit=false` → `submittable=false` (§10, §12.1). |
| R3-M07 | Active credential source recorded in `credential` table; login writes one store and deletes the other; stray entries reported; validation outcome table (mismatch, rejected, network, backend); fallback mutation refuses an unsafe existing file, `ENOENT` only creates (§8). |
| R3-M08 | Command classes A–D; selection matrix per class; `auth login` may name a new profile; offline matrix per class; env binding file for offline env-pair lookup (§5, §8, §9). |
| R3-M09 | Identity key = host slug (`[a-z0-9.-]` else `_`) + port + user id + 8-hex digest of origin and user id; full origin kept in `identity.json`; IPv6 canonical form (§8). |
| R3-M10 | API requests: same-origin only, method rules for 301/302/303/307/308, identity only from a same-origin final response; transfer requests: https, ≤5 hops, token only same-origin; upload redirects are `UploadIncomplete` (§11). |
| R3-M11 | Observations ordered by issue sequence; refill estimate `min(10/s, observed)`; cooldown waits toward 300; header-silence reset only with nothing in flight; retries pass through the admission gate; tests for delayed high sample and continuous cost-1 workload (§11, §16). |
| R3-M12 | Verify: pre-check that intent, uploaded IDs, posted attachments, and receipt sets are identical; selected attempt's attachment set must equal the receipt set (`missing`/`extra`); text compares the selected entry's `body` with `readback.body_sha256`; `unavailable` when absent (§12.2). |
| R3-M13 | Download outcome mapping incl. `unsafe_path` → 12 and `unmanaged`/`modified` → warning; `POST` timeout/undecodable 2xx → `outcome_unknown` (9), other timeouts → 4; abort envelopes carry `journal_id`, state, `posted`, and per-file results in `details` (§12.2, §12.3, §14, §7). |
| R3-M14 | `receipts show` returns `{ journal, receipt? }` with a shared `Journal` object; `receipt@1` document relationship to the envelope stated; `rubric_assessed: bool` + never-null array; `profile`/`identity` null for class A and early errors; `Posted.evidence` (§7, Appendix D). |
| R3-M15 | All-day events are one civil day from `all_day_date`; ICS emits `DTSTART;VALUE=DATE` without `DTEND`; multi-day all-day spans warned, not derived; `all_day_end_date` removed; DST and equal start/end tests (§12.5, Appendix D, §16). |
| R3-M16 | M1-a (store, identity, full schema, journal table) precedes M0-c; M0-c depends on M0-b and M1-a; three-lane round table names enum, registry, and migration owners for every round (§18). |
| R3-N01 | Uniqueness pass over the full planned set with generated names reserved; empty components → `file-<id>`; `COM0`/`LPT0` and superscript digits added (§12.3). |
| R3-N02 | Single `open` bucket definition in §12.1; name fallback over memberships only; repeated `include[]` keys in §10; `requests.cost` added; allowlist test scoped to capability-bearing URLs (§5, §6, §10, §7, §16). |
| R3-N03 | Absent and `null` collapsed in v1 models; explicit `deserialize_with` required where a distinction is needed, with a three-case fixture (§11). |

### Round 4 (v0.4 → v0.5)

| ID | Resolution |
|---|---|
| R4-B01 | Only `400`/`401`/`403`/`404`/`422` with a decodable Canvas error body are pre-commit refusals; every `5xx`, gateway error, undecodable `2xx`, timeout, or dropped connection is `outcome_unknown`; tests for commit-then-`500` and commit-then-gateway-error (§12.2, §14, §16). |
| R4-M01 | File-ID equality no longer claims authorship: state `matched`, `evidence = "history-files"`, receipt `attribution = "unproven"`; copies with other IDs are not matches; multi-match listed; test for another client using the IDs first (§12.2, Appendix D, §16). |
| R4-M02 | Admission lock per assignment plus partial unique index; owner lock acquired before the row is inserted; `planned` recovery row; recoverers hold the owner lock and re-read; tests for simultaneous confirmations, kills at every boundary, two recoverers (§12.2, §9, §16). |
| R4-M03 | Receipt built in the success transaction with `readback = null`; readback is idempotent enrichment retried by `reconcile`; export rebuilds from the row; text reference = `server_body_sha256` from POST or readback, else `unavailable` (§12.2, Appendix D). |
| R4-M04 | Pending hook exempts `outcome_unknown` journals that are superseded by a later confirmed journal or acknowledged via `receipts acknowledge`; the journal itself never changes state (§10, §5, §12.2). |
| R4-M05 | Move = three durable phases under the mutex with a recorded `move_sha256`; hashes always recorded at install; marker recovery by hash with new/old/both/neither branches and `unresolved_move`; replacement crash window classified `modified` (§12.3, Appendix D). |
| R4-M06 | Manifest DB moved into identity storage (`<identity dir>/downloads/<dest_id>.sqlite`) under the common database contract; only `dest.json` and `install.lock` live in the destination, opened via the retained root handle; M3-b-core owns migration list `dl_0001` (§9, §10, §12.3, §18). |
| R4-M07 | Per-field `field_obs` observations with three-state `Supplied<T>` deserialization; absent fields never refresh; `can_submit` freshness only from endpoints that supply it; out-of-order tests (§10, §11, §16). |
| R4-M08 | Credential activation protocol with `cleanup_pending`, crash-window behaviour, `doctor` reporting, per-identity credential lock; env binding file with lock and atomic replace; stale bindings removed (§8, §9, §16). |
| R4-M09 | Class B never uses the network (unbound env pair → actionable exit 3); `open` never fetches; `identity remove <key>` selects by operand and takes the exclusive lock directly; local credential-store exceptions enumerated; `doctor` is class B with identity-free fallback (§5, §8, §13). |
| R4-M10 | Upload `POST` is never auto-followed; its `3xx` is the completion handoff to a separate API `GET`; other redirects are `UploadIncomplete`; acceptance covers the handoff (§11, §16). |
| R4-M11 | Lower samples always applied; higher samples only when newer by issue order; cost pre-charging; refill bootstrap at 0 with a 5 s probe timer; 350 headroom target, exit at ≥ 300; header-silence rule kept (§11, §16). |
| R4-N01 | `verify@1` nullable digests; `reconcile@1` structured `candidates`; `receipt@1` fully typed incl. `attribution`, `Readback`, text `server_body_sha256`; envelope example with digest key, `requests.cost`, `stale` (Appendix D, §7). |
| R4-N02 | Warning predicate `end_at ≠ start_at` (§12.5). |
| R4-N03 | M2-b merges last and passes acceptance on merged M1-c; R5 depends on every package through R4 (§18). |

### Round 5 (v0.5 → v0.6)

| ID | Resolution |
|---|---|
| R5-B01 | No status or body is treated as proof of rollback: every non-success `POST` outcome is `outcome_unknown` with `post_status`/`post_definite`; a definite server answer triggers immediate resolution through history, and only "server answered and history shows no new attempt" yields `uploaded_not_submitted`; `--comment` capped at 65,535 characters; tests for post-commit JSON `400` (§12.2, §16). |
| R5-M01 | `field_obs` keyed by the complete normalized `entity_key` (composite for scoped rows); clock and value in one transaction; out-of-order P/Q fixture (§10, §16, §18). |
| R5-M02 | `destinations` table with root fingerprint and canonical path; moved roots rebind, copied metadata is refused; install mutex = destination lock + identity-side per-`dest_id` lock so aliases serialize (§12.3, §9, §16). |
| R5-M03 | Credential row with `active_source ∈ {keyring, file, none}` and two cleanup flags; pending cleanup is best-effort and never blocks login; logout commits `none` plus flags first, then deletes; resolution rejects `none` (§8, §16). |
| R5-M04 | Nondecreasing issue-number watermark; low samples never lower it; high samples apply only above it; 3-low → 1-lower → 2-high test (§11, §16). |
| R5-N01 | Interrupted replacement follows the ordinary clobber table; `modified` only when the selected comparison detects a difference (§12.3, §16). |
| R5-N02 | `server_match` is the matching candidate or `null`; `Candidate` without `content_matches`, `attachment_ids = []` for text/URL; `receipts export` payload; `matched` = outcome `ok`, exit 0; stored readback vs public projection; example `_local` siblings (§12.2, Appendix D). |
| R5-N03 | Class-B resolver rule (never fetch, exit 6 with the ID/URL hint); `can_submit` refresh only via the single-assignment endpoint; per-field and three-database wording aligned (§6, §10, §12.1, §13, Appendix C). |

### Round 6 (v0.6 → v0.7)

| ID | Resolution |
|---|---|
| R6-B01 | `post_definite` removed; explicit `--assume-not-submitted` after 30 min with `not_submitted_evidence = "assumed"`; absence evaluated before content/time filters; 504 → empty read → later commit fixture (§12.2, §5, §16). The v0.7 `app` heuristic was removed again in v0.8 (R7-B01). |
| R6-M01 | Equal fingerprint = same root (transparent rename, `canonical_path` updated); different fingerprint = unverified root, refused regardless of the old path; no adoption in v1; files kept unmanaged (§12.3, §16). |
| R6-M02 | Activation clears the chosen store's cleanup flag in the same transaction; cleanup never targets the active store; test for login after a failed logout (§8, §16). |
| R6-N01 | Initialization order root lock → `dest.json` → identity lock → registry; damaged metadata, crash before row insert (repaired), orphan manifest (refused) (§12.3, §16). |
| R6-N02 | `submit@1` carries `attribution`, `post_status`, `response_origin`, `server_match`, `candidates`; `response_origin` and `not_submitted_evidence` persisted; `uploaded_not_submitted` covers never-sent, app-error-empty-history, and assumed; §14 exit follows the final state (§12.2, §14, Appendix D). |

### Round 7 (v0.7 → v0.8)

| ID | Resolution |
|---|---|
| R7-B01 | No automatic negative outcome for a dispatched `POST`: `response_kind` is descriptive only; the only ways to `uploaded_not_submitted` are `never_sent` (owner-absent recovery from `uploaded`) and the explicit, labeled `--assume-not-submitted` after 30 minutes; branded JSON gateway fixture added (§12.2, §14, §16, Appendix C, D). |
| R7-M01 | Identity key in `dest.json` is validated immediately after reading it, before any lock, registry write, manifest creation, or download; fixture for identity A metadata under identity B (§12.3, §16). |

