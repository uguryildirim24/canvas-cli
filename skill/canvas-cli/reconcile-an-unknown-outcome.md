# Reconcile an unknown outcome

A submit was interrupted. Exit 9 means the tool does not know whether Canvas
recorded the attempt. A journal is left behind, and it holds the evidence.

**Do not submit again.** A second attempt can consume the user's last one, or
create a duplicate that neither of you can remove.

## Steps

1. `receipts.list` to find the open journal. `state` filters it. A journal in
   `in_progress` has a live owner: another process is still working on it, and
   you must wait rather than touch it.
2. `receipts.show` with the journal id for the full record: which files were
   uploaded, what the server answered, and what state the journal reached.
3. `submission.get` for the assignment. If Canvas shows an attempt that
   matches the journal, the work is in. Say so and stop.
4. Reconciling is a `canvas` command, not a tool: the MCP catalog is reads
   only, and resolving a journal retires evidence.
   `canvas submission reconcile <journal-id> --json` reads Canvas and never
   posts to it. It resolves the journal by evidence: matched, not submitted,
   or still unknown.
5. If it stays unknown, the user has two honest options:
   - Ask the instructor, then `canvas receipts acknowledge <journal-id>` to
     close it with the outcome recorded as unknown.
   - Resubmit deliberately, as a new attempt, with the user's explicit
     decision — see [prepare-and-submit.md](prepare-and-submit.md).

## `--assume-not-submitted`

`canvas submission reconcile` takes `--assume-not-submitted`. It records that
nothing was submitted, and it retires evidence. No tool takes it: an argument
like this belongs at the terminal, where the person types it.

Suggest it only when the user says so, after they have seen what
`submission.get` reports. The command refuses it anyway while an attempt is
visible, or while the journal is too young for its absence to mean anything —
exit 8, with the reason in `result`.

## Reporting

Say which of the three things is true: the work is in Canvas, the work is not
in Canvas, or the outcome is unknown. Never round "unknown" to either of the
others. Give the journal id and the receipt id so the user can show them to an
instructor.

## Typical calls

The reads are tools. Resolving the journal is a command.

```
receipts.list  { }
receipts.list  { "state": "outcome_unknown" }
receipts.show  { "id": "journal-..." }
submission.get { "course": "CHEM", "assignment": "Problem Set 2", "history": true }
```

```sh
canvas submission reconcile <journal-id> --json
canvas receipts acknowledge <journal-id>
```
