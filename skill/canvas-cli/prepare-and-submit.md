# Prepare and submit, with approval

The student asks to hand work in.

Submitting is `canvas submit`. It freezes a plan, prints exactly what it is
about to send, and asks for a confirmation at the terminal. That confirmation
is the person's.

## The shape of it

1. Read the assignment first. Confirm what Canvas will accept.
2. Show the student exactly what you propose to hand in: the assignment, every
   file with its size, and whether the attempt will be late.
3. Run `canvas submit`, or give the student the line to run.
4. Read the receipt back with `canvas receipts list` and
   `canvas receipts show`.

## Steps

1. Read the assignment first ([read-an-assignment.md](read-an-assignment.md)).
   Confirm it is open, is not a group assignment, and accepts what the student
   has. `canvas submission <course> <assignment> --json` shows what is already
   handed in.
2. Name exactly what the student named , files, text, HTML, or a URL. Never add
   a file the student did not name. Never change a file's contents.
3. Show it back to the student before anything runs.
4. Run the command:

```sh
canvas submit CHEM "Problem Set 2" --file ps2.pdf
canvas submit CHEM "Problem Set 2" --text answer.txt
canvas submit CHEM "Problem Set 2" --url https://example.test/work
```

   `canvas submit` prints the plan , the assignment, every file with its
   size and digest, the attempt number, the expiry , and asks. Nothing is
   sent until the person answers. `--json` prints the §7 envelope.

5. Read the outcome back: `canvas receipts list --json` for the journal,
   `canvas receipts show <id> --json` for the full record, and
   `canvas submission <course> <assignment> --json` for what Canvas holds.

## Rules

- **Never pass `--yes`.** It exists on the command line for a person who
  means it. An agent that passes it has taken the decision away from them.
- **One attempt per request.** If the command printed a receipt, the work is
  in. Do not run it again "to be sure": read the receipt, or run
  `canvas submission`.
- **Exit 9 is not a failure to retry.** It means the outcome is unknown. Go to
  [reconcile-an-unknown-outcome.md](reconcile-an-unknown-outcome.md).
- **Exit 8 is a real "no".** Attempts exhausted, a closed assignment, a
  disallowed extension, a group assignment, a file that changed mid-submit.
  Read `result` for which one, and tell the student. Do not try another route.
- **Exit 11 means the person said no.** The plan is invalidated and nothing
  was sent. Stop; do not prepare another one unasked.

## Typical commands

```sh
canvas assignment CHEM "Problem Set 2" --json
canvas submission CHEM "Problem Set 2" --json
canvas submit CHEM "Problem Set 2" --file ps2.pdf
canvas receipts list --json
canvas receipts show <receipt-id> --json
```

## Why it is this way

A submission is one attempt against one person's course record. It is
approved at the terminal where that person is, not through a host's form.
That is why `canvas mcp` serves one tool that only describes the CLI, and why
the confirmation cannot be automated away. Say so plainly if the student asks.
