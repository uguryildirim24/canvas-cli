# M10-b — New Quizzes metadata reads (owner decision, 2026-09-11)

The owner's courses use LTI quizzes. The documented New Quizzes API
(`/api/quiz/v1/...`) builds and describes quizzes and has no session,
answer, or completion endpoint; taking one happens inside the LTI tool
session, which no token reaches (§19 item 53). Neither cookie import nor
driving the undocumented player API is in this project (§1).

## Shape

Two class-D metadata reads, no cache: the LTI engine owns the state, and
the CLI never stores what it cannot reconcile.

- `canvas new-quizzes <course>` — `GET /api/quiz/v1/courses/:cid/quizzes`.
- `canvas new-quiz <course> <assignment-id>` — `GET /api/quiz/v1/courses/:cid/quizzes/:aid`.

Both print title, instructions as Markdown, due and lock times, points,
time limit, attempts, and the one-at-a-time, shuffle, and access-code
rules. A New Quiz is addressed by its assignment id.

## The assisted flow (skill only, no code)

The agent briefs from the metadata, the person opens the browser, and
pasted questions may come back for drafted answers. The agent never sees
the questions itself and never submits. `skill/canvas-cli/take-a-quiz.md`
routes Classic runs the full M10-a flow and New runs this one.
