# M10-a — Quiz taking for Classic Quizzes (owner decision, 2026-09-11)

Owner: "i want ai to solve quizzes write stuff etc". The account's quizzes
are Classic Quizzes. SPEC §19 item 52 records the decision and its three
readings; §12.7 is the contract.

## Shape

Four commands, two reads and two writes:

- `canvas quizzes <course>` — class C, `quizzes` dataset (`course:<id>`).
- `canvas quiz <course> <id|title|URL>` — class C, `quiz` detail dataset.
- `canvas quiz questions <course> <id|title|URL> [--access-code] [--yes]` — class D. Joins the live session or starts one (confirmed at the terminal or `--yes`), then prints `quiz_questions@1`.
- `canvas quiz submit <course> <id|title|URL> --answers FILE|- [--access-code] [--yes]` — class D. Freezes the canonical answer set as a `quiz_submit` operation plan, prints it, asks, posts answers, completes, all in one journal.

## Rules that must survive review

- The session start is idempotent and journaled as state (`quiz_session`), not as an operation. The answers + completion are the operation.
- An unknown outcome that the readback resolves to the frozen session, completed at its attempt with the sent answers, is `posted`/`observed` — never `matched`.
- Re-running `quiz submit` after `outcome_unknown` is a new approved operation; answers overwrite.
- Refusals before any POST: `locked`, `no_session`, `access_code`, `session_expired`, `unresolved`, `denied`, `unsupported` (LockDown, IP filter, one-question-at-a-time, no-going-back).
- The `validation_token` is stored, never printed. No new exit codes. `quiz submit` prints `operation@1` through the shared renderer.
- Every new `result` shape gets: a registry entry, a fixture, a SHAPES row, an e2e test in both modes, a README row, and a skill workflow. The schema conformance test checks every JSON snapshot.
