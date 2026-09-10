# Turn 01 — fable

## Thesis

**T1.** The CLI is the agent's surface. The browser is Rolf's surface. "Come with me" means the agent gets *context* (where Rolf is, as identifiers) and *presence* (a place to show its notes), while every *action* still goes through the CLI's API path with its journal and receipts. Remote-controlling the tab is the last resort, not the design.

**T2.** Reason: the DOM holds the same data the API returns, minus structure. `window.ENV` on an assignment page carries `COURSE_ID` and `ASSIGNMENT_ID` [S8]. Identifiers from the page plus content from the API is faster (tens of ms from cache) and cannot break on a Canvas CSS change.

## Proposals

**P1. `canvas mcp` (stdio) in the same binary.** One tool per class-C/D command, arguments = the clap flags, `outputSchema` = the Appendix D schema, `structuredContent` = the same envelope `--json` prints. Annotations: class B/C tools `readOnlyHint: true`; `submit` `destructiveHint: false, idempotentHint: false`. Cached datasets are MCP resources (`canvas://todo`, `canvas://course/45678/assignments`) with `subscriptions/listen` so a client sees `resources/updated` after `sync` [S1]. `rmcp` 3.2.0 implements the `2026-07-28` spec, stdio, Streamable HTTP, annotations, elicitation, MSRV 1.88 [S3].

**P2. The plain CLI stays first-class for shell agents.** Claude Code and Codex already have bash. Anthropic's guidance is progressive disclosure: a skill file loads on demand, MCP tool schemas are always loaded [S4]. So: ship `docs/agent/SKILL.md` and `canvas schema <command>` (prints the Appendix D JSON schema). MCP is for clients without a shell, for elicitation, and for subscriptions. Both surfaces render the same structs (§3.4).

**P3. Confirmation model: three tiers, one audit field.**

| Tier | Commands | Rule |
|---|---|---|
| Read | class B, C | never ask |
| Local write | `alias`, `receipts acknowledge`, `cache clear`, `sync` | never ask |
| Remote write | `submit`; v2 `discussion reply`, `inbox send` | always ask Rolf |

In MCP mode "ask" = form elicitation (accept/decline/cancel) [S1]; Claude Code, Cursor, and Codex (PR #17043, April 2026) support it [S2]. Without elicitation the tool returns `resultType: input_required` and never posts. The journal gets a field `confirmed_by: tty | elicitation | yes-flag`. Honest limit: an agent with shell access can run `canvas submit --yes`; no local mechanism stops that. The CLI makes the safe path easy and the unsafe path visible in `receipts list`. The real gate is the harness permission prompt on `Bash`.

**P4. Token scope is not a boundary.** Personal tokens accept `scopes[]` only when the admin enabled scoping on the default developer key; there is no read-only token type [S9]. So "no quiz taking" is policy, not capability. We must state that plainly in the report.

**P5. Bridge = MV3 extension + native messaging host, and the host is `canvas bridge`.** This is how Claude in Chrome and the Codex extension attach [S5][S6]. Native messaging keeps the service worker alive; the host manifest lives under `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/` [S7]; `canvas bridge install` registers it. The content script runs on the configured origin only, reads `window.ENV` (`current_user_id`, `context_asset_string`, `COURSE_ID`, `ASSIGNMENT_ID`) [S8], URL, and selection, and pushes a `here` record on every tab change. The host writes `<identity dir>/bridge/here.json` atomically and listens on `<data root>/bridge.sock`.

Agent-facing:

- `canvas here [--json]`: reads `here.json`. Result `{ origin, user_id, page_kind, course_id?, assignment_id?, url, selection?, observed_at, zone }`. Refuses if `user_id` differs from the active identity (exit 3). Budget < 20 ms; no socket needed.
- `canvas open ... --follow`: `open`, but a socket message navigates the current Canvas tab instead of spawning one.
- `canvas bridge status`.

**P6. Why not the alternatives.** CDP against Rolf's real profile is dead since Chrome 136: the debugging flags are ignored on the default user-data-dir [S10]. Chrome 144+ `--auto-connect` reaches the logged-in profile but shows a consent dialog per connection and a permanent "automated" banner [S11]. A separate Playwright profile forfeits the Canvas login. Claude in Chrome [S5] and the Codex extension [S6] already read pages, screenshot and click with per-site permissions, but both are per-vendor, take seconds, and cannot see canvas-cli's identifiers or journal. Our bridge is agent-agnostic and structured; theirs remain the "act in the DOM" fallback for what the API cannot do.

**P7. Presence: side panel.** `chrome.sidePanel` (Chrome 114+) [S7] shows the CLI's view of the current page (`todo` row, status, pending journals, receipts) and receives agent notes via the socket. It is also a second confirmation channel: `submit` from MCP can show the plan with Approve/Decline. Same tiers as P3.

**P8. DOM reading: yes, but bounded.** `canvas here --text` returns `innerText` of the main content region, for what the API lacks: unsaved form state, an LTI iframe title, a rubric popup. `window.ENV` is absent inside iframes [S8], so the record carries `frame: top | iframe`. Never the source for assignment text when the API has it.

**P9. Assessment zones: the bridge goes silent.** On `/quizzes/:id/take`, New Quizzes LTI launches, and LockDown Browser sessions, the content script sends only `{ zone: "assessment" }`; `here --text` refuses (exit 8). Classic quizzes *can* be taken via the API (start, answer, complete with `validation_token`) [S12]; so "no quiz taking" stays a v2+ non-goal. Discussions and inbox are conversation, not assessment; I propose lifting them in v2 (P12).

**P10. Session/cookie reuse stays excluded.** (a) The token works at Lasell. (b) A cookie-backed call needs `X-CSRF-Token` from `_csrf_token` and inherits the full web session [S8][S13]; it would give the agent a second write path that bypasses the journal. (c) Profile-bound cookie encryption is what the Chrome 136 change protects [S10]. The one future route is Better Canvas' pattern, same-origin `/api/v1` from the page [S13]: reads only, only if a token is impossible.

**P11. `canvas watch` (events).** Polling, because Live Events are admin-only [S14]. Tick every 60 s: `activity_stream/summary` and `conversations/unread_count` [S14]; on change, refresh the affected datasets and emit NDJSON events (`announcement.new`, `grade.posted`, `assignment.new`, `due.soon`, `missing.new`, `inbox.unread`). Exposed as `canvas watch --json` and as MCP resource updates. `notify` (desktop) builds on it.

**P12. Unused Canvas features, ranked.** GraphQL: one round trip for course → assignments → submissions with `missing`/`late` [S15]; measure against REST first (§12.4). Discussions: `view` renders a thread in one call; `require_initial_post` returns 403 until Rolf posts [S16]. Inbox: read with `auto_mark_as_read=false`, else a preview clears the badge [S17]. Notification preferences: a student can set `immediately` per channel [S18]. ICS feed: capability URL, never stored [S19]; we generate our own. Mobile: scheme is `canvas-courses://` [S20]; low value. Studio: no student API; drop.

**P13. Latency budget.**

| Path | Budget |
|---|---|
| `here`, cached class-C read, MCP resource | < 100 ms (§13 targets 50 ms) |
| `open --follow` | < 300 ms |
| fresh read, `sync` | 1–5 s |
| `submit` incl. confirmation | human time |
| `watch` tick | 60 s cadence |

**P14. Packages after M5.** M6-a `mcp` + `schema` + SKILL.md (dep M5-c). M6-b `watch` (dep M4-b, M6-a). M7-a bridge host + `here` + `open --follow` + extension (dep M6-a). M7-b side panel + approve channel (dep M7-a). M8-a discussions and inbox under the P3 tier (dep M6-a). M8-b GraphQL benchmark (dep M5-a).

**Open for Rolf.** Lasell's login page lists `courses.lasell.edu`, not `lasell.instructure.com` [S21]. If that is a vanity host for the same instance, the bridge allowlist and the identity origin must use the host the tab shows.

## Sources

- S1 https://modelcontextprotocol.io/specification/latest (tools, elicitation, resources, transports)
- S2 https://code.claude.com/docs/en/mcp · https://cursor.com/docs/context/mcp · https://github.com/openai/codex/pull/17043
- S3 https://crates.io/crates/rmcp · https://github.com/modelcontextprotocol/rust-sdk
- S4 https://code.claude.com/docs/en/skills · https://www.anthropic.com/engineering/code-execution-with-mcp
- S5 https://code.claude.com/docs/en/chrome · https://support.claude.com/en/articles/12902446-claude-in-chrome-permissions-guide
- S6 https://learn.chatgpt.com/docs/chrome-extension · https://learn.chatgpt.com/docs/plugins
- S7 https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging · https://developer.chrome.com/docs/extensions/develop/concepts/service-workers/lifecycle · https://developer.chrome.com/docs/extensions/reference/api/sidePanel
- S8 https://github.com/instructure/canvas-lms/blob/master/app/controllers/application_controller.rb · https://github.com/instructure/canvas-lms/blob/master/app/controllers/assignments_controller.rb · https://github.com/instructure/canvas-lms/issues/2285
- S9 https://canvas.instructure.com/doc/api/access_tokens.html · https://canvas.instructure.com/doc/api/file.developer_keys.html
- S10 https://developer.chrome.com/blog/remote-debugging-port
- S11 https://developer.chrome.com/blog/chrome-devtools-mcp-debug-your-browser-session · https://github.com/ChromeDevTools/chrome-devtools-mcp/blob/main/docs/configuration.md
- S12 https://canvas.instructure.com/doc/api/quiz_submissions.html · https://canvas.instructure.com/doc/api/new_quizzes.html
- S13 https://github.com/UseBetterCanvas/bettercanvas/blob/main/js/content.js
- S14 https://canvas.instructure.com/doc/api/file.data_service_introduction.html · https://canvas.instructure.com/doc/api/users.html · https://canvas.instructure.com/doc/api/conversations.html
- S15 https://developerdocs.instructure.com/services/canvas/basics/file.graphql
- S16 https://canvas.instructure.com/doc/api/discussion_topics.html
- S17 https://canvas.instructure.com/doc/api/conversations.html
- S18 https://canvas.instructure.com/doc/api/notification_preferences.html
- S19 https://it.umn.edu/services-technologies/how-tos/canvas-view-calendar-ical-feed-subscribe
- S20 https://community.canvaslms.com/t5/Canvas-Mobile-Forum/Canvas-Student-Android-and-iOS-URL-Schemes/m-p/529701
- S21 https://www.lasell.edu/academics/canvas-login.html

NEXT: gpt
