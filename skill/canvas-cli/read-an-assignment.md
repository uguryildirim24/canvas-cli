# Read an assignment

The user asks what an assignment asks for, when it is due, how it is graded,
or what they have already handed in.

Everything here is a `canvas` command with `--json`.

## Steps

1. Find the assignment.
   - A Canvas URL is enough: pass it as the target of
     `canvas assignment <url> --json` and give no assignment operand.
   - Otherwise `canvas assignments <course> --json`, with `--search
     <substring>` if the user gave a name.
   - `--bucket` filters by state: `open` (the default), `upcoming`, `overdue`,
     `past`, `undated`, `unsubmitted`, `ungraded`, `future`, `all`.
2. `canvas assignment <course> <assignment> --json`. The prompt comes back as
   Markdown, with the dates, the rubric, and the user's submission state.
3. `canvas submission <course> <assignment> --json` only if the user asks
   about earlier attempts. Add `--history` for every attempt.
4. `canvas grades <course> --json` if the user asks what the assignment is
   worth in the whole course. It reports the group weights Canvas reports.
5. Give the user the assignment's Canvas URL when they want to look at the
   page themselves. `canvas assignment` already carries it, and
   `canvas open assignment <course> <assignment>` opens it.

## The course material behind the assignment

An assignment prompt often points somewhere else: a course page, the
syllabus, or a discussion the class is holding.

- `canvas syllabus <course> --json` for the policy text: late work, weights,
  what counts as collaboration. It costs no request of its own.
- `canvas pages <course> --json` to see which pages exist, then
  `canvas page <course> <slug> --json` with the page's slug, its id, or a
  Canvas page URL. `--unpublished` also lists pages Canvas reports as
  unpublished.
- `canvas discussions <course> --json` and
  `canvas discussion <course> <id> --json` when the assignment *is* a
  discussion, or when the class is discussing it. Add `--replies` for the
  thread, and `--page <n>` to walk it 100 replies at a time. Reading marks
  nothing read.

Read `embedded`, `files`, and `external_links` on a page, a syllabus, or a
topic. `embedded` names content the Markdown could not show — a video, an
audio clip, an LTI tool — and each row is `reported: "unavailable"`. Say what
is there and that you cannot see it. Never claim a body is complete when
`truncated` is `true`.

`replies_coverage` says how much of a thread was actually read. `complete:
false` means the thread is not whole; `blocked: "not_requested"` means you
did not ask for replies, and `blocked: "initial_post_required"` (exit 8)
means Canvas will not show the thread until the user posts first. Never
summarize a thread you only partly read as if it were all of it.
`replies_page` and `replies_total` say which window you are looking at: an
empty `replies` beside a non-zero `replies_total` is a page past the end, not
a thread without replies. A `replies_total` of `null` means you never asked
for the thread, so you know nothing about its size; only a 0 says there are
none.

## Resolution failures

Exit 6 means zero matches or many. Do not guess. Show the candidates from
`result` and ask which one. A course string may be a numeric id, one of the
user's aliases, a Canvas URL, or a case-insensitive substring of the course
code or name — say which form you used when it was ambiguous.
`canvas alias set <name> <course>` stores a short name the user picks.

## Reporting

Quote the prompt rather than summarizing it away; a rubric line the user did
not see is a lost point. Give the due time in their local zone, and say
whether the assignment is still open.

If the assignment is a group assignment, an `external_tool`, or a quiz, say so
plainly: those cannot be submitted through this tool.

## Typical commands

```sh
canvas assignments CHEM --json
canvas assignments CHEM --bucket overdue --json
canvas assignments CHEM --search "problem set" --json
canvas assignment CHEM "Problem Set 2" --json
canvas assignment https://school.instructure.com/courses/1/assignments/500 --json
canvas submission CHEM 500 --history --json
canvas grades CHEM --json
canvas syllabus CHEM --json
canvas pages CHEM --json
canvas page CHEM lab-safety --json
canvas discussions CHEM --json
canvas discussion CHEM 55 --replies --json
canvas discussion CHEM 55 --replies --page 2 --json
```
