# Organize the week

The user asks what is due, what is missing, or what this week looks like.

Everything here is a `canvas` command. Add `--json` to read the envelope; drop
it when you only want to show the user a table.

## Steps

0. `canvas courses --json` if you do not know which courses the user is in, or
   if a course name they used does not resolve. `--all` adds completed and
   invited courses; `--favorites` keeps only their favorites.
   `canvas course <course> --json` gives one course's term, teachers, and
   reported scores.
1. `canvas todo --json` with no other argument. It merges the planner window
   with what Canvas reports as missing, so one command usually answers the
   question.
   - `--days <n>` widens or narrows the window. The default is 14 days.
   - `--missing` keeps only work Canvas reports as missing.
   - `--course <course>` limits it to one course.
2. Check `freshness`. If the `planner` or `missing` dataset is `stale` and the
   user wants current data, run `canvas sync`, then repeat step 1. `sync`
   writes the local cache and never writes to Canvas.
3. `canvas calendar --json` only if the user asks about meetings, lectures, or
   events. `canvas todo` already carries deadlines.
4. `canvas announcements --json` if the user asks whether anything changed.
   Reading an announcement never marks it read in Canvas.
   `canvas announcement <course> <id> --json` returns one announcement's
   message in full, as Markdown.
5. `canvas grades --json` if the user asks where they stand. It reports what
   Canvas reports and nothing more: `--period` takes `current`, `all`, or a
   grading-period id, and a course operand shows that course's assignment
   groups. Never compute a grade of your own.
6. `canvas inbox unread-count --json` if the user asks whether anyone is
   waiting on them. It is one number for the whole identity, and `null` means
   Canvas did not say — never read `null` as zero.
   `canvas inbox --json` then shows the conversations, with `--scope` of
   `inbox` (the default), `unread`, `sent`, or `archived`, and
   `canvas inbox show <id> --json` opens one with its messages and
   attachments.
7. `canvas discussions <course> --json` if a course's work is happening in a
   thread. `canvas discussion <course> <id> --json` shows one topic, and
   `--replies` reads the thread.

## What reading does not do

Nothing here marks anything read. Every inbox request says
`auto_mark_as_read=false`, and no discussion, announcement, or conversation
changes state because you looked at it. Say so if the user worries about it:
their unread badges are exactly as they left them.

`canvas inbox show` reports `messages_complete`. When it is `false` only the
listing row was cached, so an empty `messages` array does not mean the
conversation is empty — say the messages were not read rather than that there
are none.

## Reporting

Group by day, then by course. For every item give the course code, the title,
the local due time, and the state Canvas reports (submitted, graded, missing).

Say what is missing from the answer. `partial` names a course whose data could
not be read; report those courses by name rather than dropping them silently.

Do not compute a grade, a workload estimate, or a priority order unless the
user asks for one. If you do rank items, say the ranking is yours.

When the user wants to see something in Canvas itself, give them the link
from the answer you already have: every read carries the Canvas URL of what
it describes. `canvas open CHEM` resolves and opens one.

## Typical commands

```sh
canvas courses --json
canvas course CHEM --json
canvas todo --json
canvas todo --days 7 --json
canvas todo --missing --json
canvas todo --course CHEM --json
canvas calendar --days 7 --json
canvas announcements --since 7d --json
canvas announcement CHEM 88 --json
canvas grades --period current --json
canvas inbox unread-count --json
canvas inbox --scope unread --json
canvas inbox show 700 --json
canvas discussions CHEM --unread --json
canvas discussion CHEM 55 --json
```

## Cost

`canvas todo` from a warm cache is 0 API requests. A `canvas sync` is roughly
one request per dataset per course, so never run it in a loop. One run serves
every later read in the window. Add `--offline` when you want to be sure a
command cannot reach the network; it answers from the cache or exits 7.
