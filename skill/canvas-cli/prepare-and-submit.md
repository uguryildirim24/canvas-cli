# Prepare and submit, with approval

The user asks to hand work in. **You cannot do this alone.** Nothing reaches
Canvas without a human approval that the tool itself records. There is no
argument that asserts an approval, and there is no `--yes`.

## The shape of it

1. `submission.prepare` builds a plan: which assignment, which files or which
   text, what Canvas will accept, and what the plan's `plan_id` is. It writes
   no submission and posts nothing.
2. `submission.execute` with that `plan_id`:
   - On an **approved** plan it runs, and returns the receipt.
   - On a **prepared** plan it returns `input_required`. That is the approval
     request. Your host shows it to the user and retries the call with the
     answer.
   - When the host declares no elicitation support, it returns a domain
     refusal instead: `outcome` `refused`, exit 8, code `approval_required`.
     **Nothing was dispatched.** Tell the user to run `canvas submit` in their
     terminal, where the confirmation is a prompt.

## Steps

1. Read the assignment first ([read-an-assignment.md](read-an-assignment.md)).
   Confirm it is open, is not a group assignment, and accepts what the user
   has.
2. `submission.prepare` with the course, the assignment, and exactly what the
   user named — files, text, HTML, or a URL. Never add a file the user did not
   name. Never change a file's contents.
3. Show the plan back to the user before executing: the assignment, every file
   with its size, the attempt number, and whether the attempt will be late.
4. `submission.execute` with the `plan_id`.
5. On `input_required`, let the host collect the answer. Accept runs it;
   decline or cancel invalidates the plan and submits nothing.
6. Report the receipt: the receipt id, the attempt number, and what Canvas
   recorded.

## Rules

- **One attempt per request.** If `execute` returns a receipt, the work is in.
  Do not call it again "to be sure": read the receipt, or call
  `submission.get`.
- **Exit 9 is not a failure to retry.** It means the outcome is unknown. Go to
  [reconcile-an-unknown-outcome.md](reconcile-an-unknown-outcome.md).
- **Exit 8 is a real "no".** Attempts exhausted, a closed assignment, a
  disallowed extension, a group assignment, a file that changed mid-submit.
  Read `result` for which one, and tell the user. Do not try another route.
- **A replayed approval is not a second submission.** Executing an
  already-executed plan returns the existing journal.

## Typical calls

```
submission.prepare { "course": "CHEM", "assignment": "Problem Set 2", "files": ["ps2.pdf"] }
submission.execute { "plan_id": "plan-..." }
submission.get    { "course": "CHEM", "assignment": "Problem Set 2" }
receipts.show     { "id": "receipt-..." }
```

## The terminal route

When elicitation is unavailable, or when the user prefers it:

```sh
canvas submit CHEM "Problem Set 2" --file ps2.pdf
canvas receipts list
```

`canvas submit` asks for confirmation at the terminal and writes the same
journal and receipt.
