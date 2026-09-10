# M4-b — `announcements`, `announcement`, `calendar`, ICS, `sync --full` (Cursor Auto, lane w3)

Read `docs/SPEC.md` §5 (`announcements`, `announcement`, `calendar`,
`sync`, "Behaviour notes"), §6 (`announcement <url>`), §7 (raw-output
exception for `--ics -`), §10 (datasets `announcements`, `calendar_events`,
window scope keys with the context hash; hit predicate window rule), §12.1
(planner kinds, shared with `calendar`), §12.5 (all of it), §12.6 (all of
it), §14, §16 rows 2–3, Appendix A (no ICS crate is pinned: `canvas-core::ics`
writes RFC 5545 by hand), Appendix B, Appendix D (`announcements@1`, `announcement@1`, `calendar@1`,
`sync@1`). Existing code: `canvas-api` models (announcements, calendar
events, discussion topics), `canvas-core::sync` (M1-b `sync` for its
datasets; M1-c `planner`; M3-a `folders`/`files`/`modules`),
`canvas-core::ics` (M3-b-core: escaping, folding, all-day rules),
`canvas-core::markdown`, `canvas-core::resolve`,
`crates/canvas-cli/src/output`. Read their public APIs first.

## Round-4 interface (do this first)
You are the **enum owner** this round. Before anything else, make sure the
command enum and dispatch have entries for `grades` (M4-a, lane w1) and
`download` (M3-b, lane w2) as well as your own commands (stubs that exit 2
`not implemented` are enough where a command is still missing), commit
that alone, run the gates, and reply exactly `DONE R4-enum`. Then continue
below without waiting for an answer. Lane w1 owns the schema registry this
round and adds `announcements@1`, `announcement@1`, `calendar@1` with
placeholder fixtures; `git merge main` when Claude tells you it landed and
replace the placeholders.

## Deliverables
1. Datasets in `canvas-core::sync`: `announcements(window, contexts)`
   (batches of ≤10 `context_codes[]`, paginated, `start_date`/`end_date`;
   a non-throttle `403` on a batch triggers one request per course in that
   batch, courses that still fail are recorded in `partial[]`; batches are
   stored or isolated independently), `calendar_events(window, contexts)`
   (`type=event`, `user_<id>` plus every active `course_<id>`, batches of
   ≤10, all pages). Scope keys carry the window and the sha256 of the
   sorted contexts; coverage = the window.
2. Commands: `announcements [<course>] [--since DURATION] [--unread]`
   (default 14 days; `--unread` local on `read_state`; nothing is marked
   read), `announcement <course> <id>` and `announcement <url>` (`GET
   /courses/:cid/discussion_topics/:id`; message as Markdown; bare IDs
   refused), `calendar [--days N] [--course] [--ics PATH|-] [--alarm
   DURATION]` per §12.5 (planner window + calendar events, de-duplicated
   by calendar event ID with the events representation winning; every
   all-day event is one civil day `all_day_date`, with the warning when
   `end_at ≠ start_at`).
3. ICS per §12.5 on top of `canvas-core::ics`: `VCALENDAR` with
   `PRODID`/`VERSION`, one `VEVENT` per item, `UID =
   canvas-<kind>-<id>@<identity-key>`, `DTSTAMP`, `DTSTART` in UTC or
   `DTSTART;VALUE=DATE` with **no `DTEND`** for all-day events, `DTEND` for
   timed events with an end, no `DURATION` for point deadlines, `SUMMARY =
   [CODE] title`, `URL`, `DESCRIPTION`, §3.3.11 escaping, CRLF, 75-octet
   folding, `VALARM` from `--alarm`. `--ics -` streams raw (no envelope;
   `--json` with `--ics -` is exit 2).
4. `sync --full`: assemble the full refresh (courses, assignments,
   submissions, missing, default planner window, enrollment grades,
   announcements, plus folders, files, modules, module items, calendar
   events) on the M1-b `sync` command; `sync@1` per dataset with request
   counts and errors.
5. Renderers and schemas `announcements@1`, `announcement@1`, `calendar@1`
   with fixtures; sorts per Appendix D.
6. Tests (§16): batch isolation (one batch fails, the others are served;
   per-course retry after a batch `403`; `partial[]` names the courses);
   window hit predicate with a different context hash; `--unread`;
   `announcement` by URL and refusal of a bare ID; calendar de-duplication
   and event fields winning; ICS escaping, folding, all-day west-of-UTC,
   all-day across DST, equal start and end (one day, no `DTEND`), the
   longer-span warning, `VALARM`; `--ics -` raw output and the `--json`
   conflict; `sync --full` request counts on fixtures; snapshot tests in
   human and `--json` mode.
7. Manual client notes: import and re-import the generated ICS in Apple
   Calendar and Google Calendar (changed due date, removed event, all-day
   west of UTC, all-day across DST, equal start and end) and write the
   observed behaviour to `docs/ics-clients.md`. This is the one file under
   `docs/` you may create. No zero-lag or auto-refresh claim.

## Rules
- You own the two datasets above, the `announcements`/`announcement`/
  `calendar` command modules and renderers, the ICS writer glue, the
  `sync --full` assembly, and the command enum and dispatch this round.
- You are the **migration owner** this round; none is expected. If you
  must add one, number it after the last existing migration and say so in
  your final message.
- Work on branch `lane/w3` in this worktree. Commit as you go. Before
  reporting, `git merge main` (resolve, rerun gates). Do not push.
- Do not touch `docs/` (except `docs/ics-clients.md`) or `tasks/`.

## Gates
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
```
Finish with `git status --short` and reply exactly: `DONE M4-b`
