# Prepare and submit, with approval

The user asks to hand work in. **You cannot do this over MCP at all.** The
tool catalog is reads only: there is no prepare tool, no execute tool, and no
argument anywhere that sends bytes to Canvas.

Submitting is a `canvas` command the person runs, or that you run for them in
a terminal. It asks for a confirmation at that terminal, and the confirmation
is the person's.

## The shape of it

1. Read the assignment over MCP first. Confirm what Canvas will accept.
2. Show the user exactly what you propose to hand in: the assignment, every
   file with its size, and whether the attempt will be late.
3. Run `canvas submit` in a terminal, or give the user the line to run. It
   freezes the same plan, prints it, and asks for a confirmation.
4. Read the receipt back over MCP with `receipts.list` and `receipts.show`.

## Steps

1. Read the assignment first ([read-an-assignment.md](read-an-assignment.md)).
   Confirm it is open, is not a group assignment, and accepts what the user
   has. `submission.get` shows what is already handed in.
2. Name exactly what the user named — files, text, HTML, or a URL. Never add
   a file the user did not name. Never change a file's contents.
3. Show it back to the user before anything runs.
4. Run the command:

```sh
canvas submit CHEM "Problem Set 2" --file ps2.pdf
canvas submit CHEM "Problem Set 2" --text answer.txt
canvas submit CHEM "Problem Set 2" --url https://example.test/work
```

   `canvas submit` prints the plan — the assignment, every file with its
   size and digest, the attempt number, the expiry — and asks. Nothing is
   sent until the person answers. `--json` prints the same envelope the read
   tools return.

5. Read the outcome back: `receipts.list` for the journal, `receipts.show`
   with its id for the full record, `submission.get` for what Canvas holds.

## Rules

- **Never pass `--yes`.** It exists on the command line for a person who
  means it. An agent that passes it has taken the decision away from them.
- **One attempt per request.** If the command printed a receipt, the work is
  in. Do not run it again "to be sure": read the receipt, or call
  `submission.get`.
- **Exit 9 is not a failure to retry.** It means the outcome is unknown. Go to
  [reconcile-an-unknown-outcome.md](reconcile-an-unknown-outcome.md).
- **Exit 8 is a real "no".** Attempts exhausted, a closed assignment, a
  disallowed extension, a group assignment, a file that changed mid-submit.
  Read `result` for which one, and tell the user. Do not try another route.
- **Exit 11 means the person said no.** The plan is invalidated and nothing
  was sent. Stop; do not prepare another one unasked.

## Typical calls

The reads are tools. The write is a command.

```
assignment.get { "course": "CHEM", "assignment": "Problem Set 2" }
submission.get { "course": "CHEM", "assignment": "Problem Set 2" }
receipts.list  { }
receipts.show  { "id": "receipt-..." }
```

```sh
canvas submit CHEM "Problem Set 2" --file ps2.pdf
canvas receipts list
canvas receipts show <receipt-id>
```

## Why it is this way

The owner narrowed the MCP catalog to reads on 2026-09-10. A submission is
one attempt against one person's course record, and it is approved at the
terminal where that person is, not through a host's form. Say so plainly if
the user asks why you cannot just do it.
