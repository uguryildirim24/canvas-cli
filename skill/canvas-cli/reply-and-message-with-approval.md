# Reply and message, with approval

The user asks to answer a discussion, write to somebody, or answer a message
in the Canvas inbox. **You cannot do any of it alone.** Nothing reaches Canvas
without a human approval that the tool itself records. There is no argument
that asserts an approval.

## What these writes are

A discussion reply is a **public post in a course**. Classmates and the
instructor read it, and it carries the user's name. A conversation is private
mail, but it is still written in the user's voice to a real person.

Because of that, the course-policy boundary is the user's, not yours:

- **An approval to post is not permission for AI-generated academic work.**
  Many courses forbid it, and the rule is the course's. Write what the user
  asks for, show it to them, and let them decide.
- **Never write a placeholder.** A topic with an initial-post gate hides the
  replies until the user posts. Do not post anything to open it. The tool
  refuses this, and so should you.
- **Never invent a recipient.** Send only to the user ids the user named.
- **Say what it is before it goes.** Show the exact text and every attachment
  before the approval, not after.

## The shape of it

1. A `*.prepare` tool freezes a plan: the exact thread or recipients, the
   exact bytes of the message, and every attachment with its size and digest.
   It reads Canvas to check the target. It posts nothing.
2. The matching `*.execute` with that `plan_id`:
   - On an **approved** plan it sends, and returns the operation journal.
   - On a **prepared** plan it returns `input_required` with a `requestState`
     and one `elicitation/create` request. That is the approval request. Your
     host shows it to the user and retries the same call under a new id,
     echoing `requestState` and putting the answer in `inputResponses`.
   - When the host declares no elicitation support, it returns a domain
     refusal instead: `outcome` `refused`, exit 8, with
     `result.details.reason` = `approval_required` and the handle. **Nothing
     was dispatched.** Tell the user to run the command in their terminal,
     where the confirmation is a prompt.

## Steps

1. Read the thread first. `discussion.get` with `replies`, or `inbox.get`, so
   the user's answer is an answer to what is actually there.
2. Call the prepare tool with exactly what the user named. Never add a
   recipient, an attachment, or a sentence they did not ask for.
3. Show the plan back: the thread or the recipients, the whole message text,
   and every attachment.
4. Call the execute tool with the `plan_id`.
5. On `input_required`, let the host collect the answer. Accept sends it;
   decline or cancel invalidates the plan and sends nothing.
6. Report what came back: the state, the `attribution`, and the `delivery`
   field.

## What you may claim afterwards

`attribution` is evidence, not confidence, and `delivery` says whether the
outcome can be observed at all:

| `attribution` | What is true |
|---|---|
| `accepted` | Canvas answered 2xx and named the object it created. |
| `observed` | A later read of the thread shows that same object. |
| `unproven` | A message with the same digest is there, and nothing links it to this request. |
| `none` | Nothing links this journal to an object in Canvas. |

**A conversation Canvas accepted is not delivered mail.** Both inbox writes
carry `delivery: "not_observable"`: Canvas never reports that a person
received or read a message. Say "Canvas accepted it". Never say "it was
delivered", "they got it", or "they have seen it".

## Rules

- **One message per request.** If execute returns a journal in state `posted`,
  the message is in. Do not call it again "to be sure": call
  `operation.status`.
- **Exit 9 means the outcome is unknown, not failed.** State
  `outcome_unknown`. Nothing is ever resent automatically, and you must not
  resend either. Call `operation.reconcile`, which reads the thread back.
  `assume_not_posted` records that nothing was posted, and it is refused while
  a matching message is visible or the journal is younger than 30 minutes.
- **Exit 8 is a real "no".** `group_write` (a group discussion),
  `locked` (a closed topic), `initial_post_required` (the gate above),
  `unresolved` (an entry or a recipient that is not there),
  `denied` (a course or conversation this identity cannot see), and
  `unsupported` (an attachment on a discussion reply, which this version does
  not send). Read `result.details.reason`, tell the user, and do not try
  another route.
- **A replayed approval is not a second message.** Executing an
  already-executed plan returns that journal's `operation@1` envelope with
  `replayed: true`. No second message is created.
- **A pending write makes a read uncertain.** While `pending` is true on
  `discussion.get`, `inbox.list`, `inbox.get`, or `inbox.unread_count`, a
  write of the user's own is unresolved. Say so instead of reporting the
  thread as settled.

## Typical calls

```
discussion.reply.prepare { "course": "CHEM", "discussion": "3001", "text": "..." }
discussion.reply.execute { "plan_id": "plan-..." }
inbox.send.prepare       { "recipients": ["77"], "subject": "Lab partner", "text": "..." }
inbox.send.execute       { "plan_id": "plan-..." }
inbox.reply.prepare      { "conversation_id": "700", "text": "..." }
inbox.reply.execute      { "plan_id": "plan-..." }
operation.status         { "journal_id": "..." }
operation.reconcile      { "journal_id": "..." }
```

## The terminal route

When elicitation is unavailable, or when the user prefers it:

```sh
canvas discussion reply CHEM 3001 --text "..."
canvas inbox send --to 77 --subject "Lab partner" --text "..."
canvas inbox reply 700 --text "..."
canvas operation status <journal-id>
```

Each one prints the plan, asks for confirmation at the terminal, and writes
the same journal and receipt.
