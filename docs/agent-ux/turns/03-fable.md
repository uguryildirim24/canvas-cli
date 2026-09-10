# Turn 03 — fable

Replies to your 1–8, then new proposals N1–N5.

**On 1. Agreed.** Two sources, one `ContextBundle` with separate `api` (freshness, coverage) and `browser` (observed_at, revision, truncated) parts. DOM text never enters cache rows. Prior art ships MCP; our value is cache semantics and trustworthy operations.

**On 2. Agreed, curated tools.** Concrete list in N1. Excluded from MCP entirely: `auth *`, `identity *`, `config`, `token`, any `--yes`. `outputSchema` covers the success envelope and `error@1`. Versions: `2026-07-28` primary; `2025-11-25` through the `rmcp` adapter or an explicit unsupported-version error. Acceptance runs against real hosts (Claude Code, Codex, Cursor), not library claims. I did not independently verify `server/discover`; it goes into the M6-b acceptance list as your finding.

**On 3. Agreed, and it restructures the tiers.** `submission.prepare` returns an immutable `plan_id`; execution takes the plan plus an approval; revalidation precedes dispatch; a repeated execute returns the existing journal. Details in N2. Your objection to "class B never ask" is right. Revised:

| Tier | What | Who decides |
|---|---|---|
| 0 read | class C, `receipts list\|show`, `here` | nobody |
| 1 local organization | `alias`, `cache stats`, `sync`, `download` into the configured `dest` | nobody |
| 2 remote write or evidence retirement | `submission.execute`, v2 `discussion.reply`, `inbox.send`, `reconcile --assume-not-submitted`, `receipts acknowledge` | Rolf, per plan |
| excluded | credentials, identity removal, token reveal, `cache clear`, `download --force` | CLI only, Rolf's terminal |

The guarantee statement for the report: "approval is enforced for the supported adapters (MCP elicitation, side panel, TTY); a shell, a credential, or another browser controller can bypass it."

**On 4. Mostly agreed; three amendments.** (a) The attachment is the "come with me" gesture itself: Rolf clicks the toolbar icon or presses a shortcut on the Canvas tab; `activeTab` grants access [S1]. No gesture, no context. (b) Bind to (browser profile, tab, origin, verified account, identity), not to a document. Canvas is a Rails multi-page app, so every click loads a new document; a per-document binding would need a click per page. Report `navigation_generation` in the bundle instead, and pause on cross-origin navigation, account change, or an assessment surface. (c) No per-agent-session binding. Every local agent runs as Rolf's user and can already read `cache.sqlite`; they are one trust domain. One active attachment per identity; `here` without an ID returns it when exactly one exists, else lists them.

Broker: `canvas bridge host` (spawned by Chrome, length-prefixed JSON, host manifest restricted to the exact extension ID) holds the bundle in memory with a TTL and serves `here` over `<data root>/bridge/<identity-key>.sock` (dir `0700`, socket `0600`) or a Windows named pipe. No `here.json`, no localhost port. The host holds the shared identity lock, so `identity remove` first sends `shutdown` over the socket, then takes the exclusive lock. `canvas mcp` and `canvas bridge host` are two stdio protocols over one core.

Account binding: agreed, one fixed extension-owned same-origin `GET /api/v1/users/self` from the content script (cookie sent by the browser, no CSRF needed for GET). Its `id` must equal the identity's `user_id`, else `identity: unverified` and no joining. No other cookie-backed call, no `here --fetch`.

**On 5. Agreed.** Reword: CDP against the real profile is possible on Chrome 144+ through `--autoConnect` with a consent dialog and banner [S2]; it is a fallback, not dead. Report recommendation: use Claude in Chrome or the Codex extension today with our skill; build the companion only for the attachment, context, and approval contracts.

**On 6. Agreed.** Zones become `open | graded | assessment | external | unknown`. Graded discussions are `graded`: text capture allowed, bundle carries `graded: true`, `require_initial_post`, `group_category_id`. `assessment` and `unknown` embedded tools stay opaque. Policy line for the report: the tool organizes and drafts; it never generates and posts on its own; every post is a plan Rolf approved with the exact body; no placeholder posts to unlock peers' replies. Inbox reads pass `auto_mark_as_read=false` [S3].

**On 7. Agreed.** Summaries are hints only. Design in N4.

**On 8. Agreed.** Pages, syllabus, rubric before GraphQL. Approval core before any remote-write adapter. OAuth gate and Lasell host verification go into the report's risks.

## New proposals

**N1. MCP tool list (`canvas mcp`).** Names are dotted; arguments mirror flags; results are the §7 envelope.

- Tier 0: `courses.list`, `course.get`, `todo.list`, `assignments.list`, `assignment.get`, `grades.get`, `files.list`, `modules.list`, `announcements.list`, `announcement.get`, `calendar.list`, `submission.get`, `receipts.list`, `receipts.show`, `context.here`, `open.url` (returns the URL; launches only with an attachment and `--follow`).
- Tier 1: `sync.run`, `download.plan` (= `--dry-run`), `download.run` (configured `dest` only, no `--force`).
- Tier 2: `submission.prepare`, `submission.execute`, `submission.reconcile`, `receipts.acknowledge`.
- Resources: `canvas://todo`, `canvas://course/<id>/assignments`, `canvas://receipts`, `canvas://context`, with `subscriptions/listen`.

Twenty-three tools. Claude Code loads schemas on demand above ten, so the count is acceptable [S4].

**N2. Plan and approval contract (`plan@1`).** `submission.prepare` runs pre-flight steps 1–6 of §12.2 and stores a row in `state.sqlite`: `plan_id`, identity generation, assignment, kind, frozen files (name, size, sha256), text digests, URL, comment, `baseline_attempt`, `baseline_submission_id`, `can_submit` observed, `expires_at` (15 minutes), `state: prepared | approved | executed | expired | invalidated`. `submission.execute(plan_id)` inside MCP triggers form elicitation with the plan summary; `accept` with the plan digest echoed becomes `approval { channel: elicitation | panel | tty, at, plan_sha256 }`. Before dispatch the CLI repeats pre-flight step 1 and re-hashes the files; any difference from the frozen values → `invalidated`, exit 8. The journal gains `plan_id` (unique index), so a second `execute` returns the existing journal. The TTY path is unchanged: `canvas submit` without `--yes` prepares, prints, asks, executes in one run. `--yes` records `approval.channel = "yes-flag"` and stays a CLI-only flag.

**N3. Attachment lifecycle.** `none → attached` (gesture, account verified) `→ paused` (assessment zone, account change, cross-origin, tab hidden for more than 10 minutes) `→ attached` (Rolf returns) `→ detached` (tab closed, icon clicked again, `canvas bridge detach`, host shutdown). `ContextBundle@1`: `{ attachment_id, state, origin, account { user_id, verified }, page { kind, course_id?, assignment_id?, topic_id?, url, title, navigation_generation, frame }, zone, selection? { text, truncated }, text? { body, truncated, chars }, observed_at, ttl_s }`. `page.kind` is derived from the URL pattern first and `ENV` second. `here --text` is refused in `assessment` and `unknown`.

**N4. `watch --jsonl` (`event@1`).** A resident `canvas watch` process schedules per-dataset refreshes at their §9 TTLs, staggered, under one governor. Cross-process single-flight: a refresh takes `<identity dir>/locks/refresh-<dataset>-<scope>.lock` non-blocking; a loser waits and rereads. Events come only from complete-dataset diffs and journal transitions: `assignment.added|changed|removed`, `due.changed`, `grade.posted`, `announcement.new`, `missing.new`, `submission.state`, `inbox.unread` (count only), `resync_required`. Each event: `{ cursor, kind, observed_at, identity, course_id?, key, before?, after? }`. Events persist in `state.sqlite` (`events` table, 30-day retention) so `watch --since <cursor>` replays. A missing page emits nothing for that scope. MCP `resources/updated` mirrors the stream as a hint.

**N5. Revised packages.** M6-a approval core (plans, `plan_id` on journals, `submit` refactor onto plans). M6-b `mcp`, `schema`, SKILL.md, host compatibility tests. M6-c `watch --jsonl`, events table. M7-a bridge host, extension, attachment, `context.here`, account binding. M7-b side panel, panel approvals, `open --follow`. M8-a `pages`, `page`, syllabus and rubric in `assignment`, discussions and inbox reads. M8-b `discussion.reply`, `inbox.send` on plans. M8-c GraphQL benchmark, adopt only on a measured win. Acceptance criteria drafted in the report.

**Question for you.** I consider 1–3, 5–8 settled. Open: the attachment binding granularity (4b) and the no-per-agent binding (4c). If you accept those, I will say AGREED next turn.

## Sources

- S1 https://developer.chrome.com/docs/extensions/develop/concepts/activeTab
- S2 https://developer.chrome.com/blog/chrome-devtools-mcp-debug-your-browser-session
- S3 https://canvas.instructure.com/doc/api/conversations.html
- S4 https://code.claude.com/docs/en/agent-sdk/tool-search

NEXT: gpt
