# Organize the week

The user asks what is due, what is missing, or what this week looks like.

## Steps

0. `courses.list` if you do not know which courses the user is in, or if a
   course name they used does not resolve. `all: true` adds completed and
   invited courses; `favorites: true` keeps only their favorites. Use
   `course.get` for one course's term, teachers, and reported scores.
1. `todo.list` with no arguments. It merges the planner window with what
   Canvas reports as missing, so one call usually answers the question.
   - `days` widens or narrows the window. The default is 14 days.
   - `missing: true` keeps only work Canvas reports as missing.
   - `course` limits it to one course.
2. Check `freshness`. If the `planner` or `missing` dataset is `stale` and the
   user wants current data, call `sync.run` once and repeat step 1.
3. `calendar.list` only if the user asks about meetings, lectures, or events.
   `todo.list` already carries deadlines.
4. `announcements.list` if the user asks whether anything changed. Reading an
   announcement here never marks it read in Canvas. `announcement.get` returns
   one announcement's message in full, as Markdown.
5. `grades.get` if the user asks where they stand. It reports what Canvas
   reports and nothing more: a `period` of `current`, `all`, or a
   grading-period id, and a `course` for one course's assignment groups.
   Never compute a grade of your own.

## Reporting

Group by day, then by course. For every item give the course code, the title,
the local due time, and the state Canvas reports (submitted, graded, missing).

Say what is missing from the answer. `partial` names a course whose data could
not be read; report those courses by name rather than dropping them silently.

Do not compute a grade, a workload estimate, or a priority order unless the
user asks for one. If you do rank items, say the ranking is yours.

When the user wants to see something in Canvas itself, `open.url` returns the
canonical URL for a course, an assignment, a file, or an announcement. It does
not open a browser: give the user the link.

## Typical calls

```
courses.list {}
course.get { "course": "CHEM" }
todo.list {}
todo.list { "days": 7 }
todo.list { "missing": true }
todo.list { "course": "CHEM" }
sync.run {}
calendar.list { "days": 7 }
announcements.list { "since": "7d" }
announcement.get { "course": "CHEM", "id": "88" }
grades.get { "period": "current" }
open.url { "target": "CHEM" }
```

## Cost

`todo.list` from a warm cache is 0 API requests. `sync.run` is roughly one
request per dataset per course, so do not call it in a loop. One `sync.run`
serves every later read in the window.
