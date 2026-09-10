# Read an assignment

The user asks what an assignment asks for, when it is due, how it is graded,
or what they have already handed in.

## Steps

1. Find the assignment.
   - A Canvas URL is enough: pass it as `course` to `assignment.get` and leave
     `assignment` out.
   - Otherwise `assignments.list` with the course, and a `search` substring if
     the user gave a name.
   - `bucket` filters by state: `open` (the default), `upcoming`, `overdue`,
     `past`, `undated`, `unsubmitted`, `ungraded`, `future`, `all`.
2. `assignment.get` with the course and the assignment. The prompt comes back
   as Markdown, with the dates, the rubric, and your submission state.
3. `submission.get` only if the user asks about earlier attempts. Add
   `history: true` for every attempt.
4. `grades.get` with the course if the user asks what the assignment is worth
   in the whole course. It reports the group weights Canvas reports.
5. `open.url` when the user wants to look at the page themselves. It returns
   the canonical Canvas URL and opens nothing.

## Resolution failures

Exit 6 means zero matches or many. Do not guess. Show the candidates from
`result` and ask which one. A course string may be a numeric id, one of the
user's aliases, a Canvas URL, or a case-insensitive substring of the course
code or name — say which form you used when it was ambiguous.

## Reporting

Quote the prompt rather than summarizing it away; a rubric line the user did
not see is a lost point. Give the due time in their local zone, and say
whether the assignment is still open.

If the assignment is a group assignment, an `external_tool`, or a quiz, say so
plainly: those cannot be submitted through this tool.

## Typical calls

```
assignments.list { "course": "CHEM" }
assignments.list { "course": "CHEM", "bucket": "overdue" }
assignments.list { "course": "CHEM", "search": "problem set" }
assignment.get { "course": "CHEM", "assignment": "Problem Set 2" }
assignment.get { "course": "https://school.instructure.com/courses/1/assignments/500" }
submission.get { "course": "CHEM", "assignment": "500", "history": true }
grades.get { "course": "CHEM" }
open.url { "target": "https://school.instructure.com/courses/1/assignments/500" }
```
