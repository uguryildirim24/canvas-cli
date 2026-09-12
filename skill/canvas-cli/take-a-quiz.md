# Take a quiz, with approval

The user asks to answer a Classic Quiz. Quizzes are read and written through
`canvas` commands: the censored questions come from the API, the agent
computes the answers, and the person approves the exact answer set at the
terminal before anything is sent.

## Which kind of quiz

```sh
canvas quizzes CHEM --json
canvas new-quizzes CHEM --json
```

The first listing names Classic Quizzes, the second New Quizzes. A Classic
Quiz runs the full flow below. A New Quiz is taken in the browser — no
command here takes it, because taking one happens inside the LTI tool
session. Brief the user from its metadata instead:

```sh
canvas new-quiz CHEM 9013 --json
```

That names the rules: the attempts, the time limit, the instructions. When
the user opens it in the browser, they can paste the questions back here
and the agent drafts the answers — but the agent never sees the questions
itself, and it never submits.

When the quiz is in neither listing, read the assignment: any other LTI
tool shows an `external_tool` submission type, and it is taken in the
browser too. Say so and stop.

```sh
canvas assignment CHEM "Week 3 Reading Quiz" --json
```

## Read the questions (Classic only)

`canvas quiz` names the rules: the time limit, the attempts, whether going
back is allowed, and whether the quiz is locked. A locked quiz, a quiz that
shows one question at a time, a no-going-back quiz, and a LockDown or
IP-filtered quiz are refused (exit 8). Those need the browser. Say so and
stop.

## Read the questions

```sh
canvas quiz questions CHEM "Week 3 Reading Quiz" --json
```

This joins the session in progress, or starts one. Starting begins an
attempt and may start the clock. The command asks at the terminal first, or
records the start with `--yes`. Only start when the user asked to take the
quiz now. A session already in progress is a plain read and asks nothing.

The answer lists every question with its id, its type, its points, and its
choices. Correctness never appears — the agent reads the choices, never the
key.

## Answer and turn in

Write the answers as JSON, one entry per question:

```json
[
  { "id": 901, "answer": 4811 },
  { "id": 902, "answer": "The flask was wet, which dilutes the titrant; ..." },
  { "id": 903, "answer": 4901 },
  { "id": "904", "answer": [5101, 5102] }
]
```

The value follows the question type: a choice id for multiple choice and
true/false, an array of choice ids for multiple answers, text for an essay
or a short answer, a number for a formula or numerical question, and a map
for blanks, dropdowns, and matching. A question left out is a blank answer.

Then show the file back and give the user the line to run:

```sh
canvas quiz submit CHEM "Week 3 Reading Quiz" --answers answers.json
```

`quiz submit` freezes the answer set, prints every question with the answer
it will send, and asks. Nothing is sent until the person answers. The
answers go in one request and the completion in a second one, and the journal
covers both.

## Rules

- **The owner decides what help is allowed.** When the user asks for
  AI-written answers, write them. Never lecture about academic integrity,
  and never add a warning they did not ask for.
- **Never pass `--yes`.** It exists for a person who means it. An agent that
  passes it has taken the decision away from them. The `questions` start is
  the same: ask the person, or give them the line with `--yes` to run.
- **One quiz per request.** If the command printed a journal in state
  `posted`, the quiz is in. Do not run it again "to be sure": run
  `canvas operation status`.
- **Exit 9 means the outcome is unknown, not failed.** A process that died
  between the answers and the completion leaves exactly this. State
  `outcome_unknown`. Nothing is ever resent automatically, and you must not
  resend either. `canvas operation reconcile <journal-id>` reads the session
  back: a completed session with the sent answers resolves it. An open
  session with the answers recorded stays unknown until the quiz is turned
  in — preparing again and approving again is a new operation, and the
  answers overwrite, never duplicate.
- **Exit 8 is a real "no".** `locked` (a quiz this identity cannot start),
  `no_session` (no session in progress; read the questions first),
  `access_code` (a wrong code), `unresolved` (an answer for a question the
  session does not hold), `unsupported` (LockDown, IP filter, one question
  at a time, no going back), and `denied` (a quiz this identity cannot see).
  Read `result` for the reason, tell the user, and do not try another route.
- **A timed quiz keeps its own clock.** `end_at` in the questions answer is
  when the attempt is overdue. Say it plainly when it matters.

## Typical commands

```sh
canvas quizzes CHEM --json
canvas quiz CHEM "Week 3 Reading Quiz" --json
canvas quiz questions CHEM "Week 3 Reading Quiz" --json
canvas quiz submit CHEM "Week 3 Reading Quiz" --answers answers.json
canvas operation status <journal-id> --json
canvas operation reconcile <journal-id> --json
```
