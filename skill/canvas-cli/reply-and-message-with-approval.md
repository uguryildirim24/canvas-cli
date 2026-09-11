# Reply and message, with approval

The user asks to answer a discussion, write to somebody, or answer a message
in the Canvas inbox. **You cannot do any of it over MCP.** The tool catalog is
reads only: there is no prepare tool, no execute tool, and no argument
anywhere that posts.

Each write is a `canvas` command the person runs, or that you run for them in
a terminal. It prints the exact bytes and asks for a confirmation there.

## What these writes are

A discussion reply is a **public post in a course**. Classmates and the
instructor read it, and it carries the user's name. A conversation is private
mail, but it is still written in the user's voice to a real person.

Because of that, the course-policy boundary is the user's, not yours:

- **An approval to post is not permission for AI-generated academic work.**
  Many courses forbid it, and the rule is the course's. Write what the user
  asks for, show it to them, and let them decide.
- **Never write a placeholder.** A topic with an initial-post gate hides the
  replies until the user posts. Do not post anything to open it. The command
  refuses this, and so should you.
- **Never invent a recipient.** Send only to the user ids the user named.
- **Say what it is before it goes.** Show the exact text and every attachment
  before the command runs, not after.

## Steps

1. Read the thread first over MCP. `discussion.get` with `replies: true`, or
   `inbox.get`, so the user's answer is an answer to what is actually there.
2. Draft exactly what the user named. Never add a recipient, an attachment, or
   a sentence they did not ask for.
3. Show it back: the thread or the recipients, the whole message text, and
   every attachment.
4. Run the command, or give the user the line to run:

```sh
canvas discussion reply CHEM 3001 --text "..."
canvas inbox send --to 77 --subject "Lab partner" --text "..."
canvas inbox reply 700 --text "..."
```

   Each one freezes a plan, prints the exact thread or recipients, the exact
   bytes, and every attachment with its digest, and asks. Nothing is sent
   until the person answers.

5. Read what came back: `canvas operation status <journal-id>` for the state,
   the `attribution`, and the `delivery` field. `--json` prints the same
   envelope the read tools return.

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

- **Never pass `--yes`.** It exists for a person who means it. An agent that
  passes it has taken the decision away from them.
- **One message per request.** If the command printed a journal in state
  `posted`, the message is in. Do not run it again "to be sure": run
  `canvas operation status`.
- **Exit 9 means the outcome is unknown, not failed.** State
  `outcome_unknown`. Nothing is ever resent automatically, and you must not
  resend either. `canvas operation reconcile <journal-id>` reads the thread
  back. `--assume-not-posted` records that nothing was posted, and it is
  refused while a matching message is visible or the journal is younger than
  30 minutes.
- **Exit 8 is a real "no".** `group_write` (a group discussion),
  `locked` (a closed topic), `initial_post_required` (the gate above),
  `unresolved` (an entry or a recipient that is not there),
  `denied` (a course or conversation this identity cannot see), and
  `unsupported` (an attachment on a discussion reply, which this version does
  not send). Read `result.details.reason`, tell the user, and do not try
  another route.
- **Exit 11 means the person said no.** The plan is invalidated and nothing
  was sent.
- **A pending write makes a read uncertain.** While `pending` is true on
  `discussion.get`, `inbox.list`, `inbox.get`, or `inbox.unread_count`, a
  write of the user's own is unresolved. Say so instead of reporting the
  thread as settled.

## Typical calls

The reads are tools. The writes are commands.

```
discussion.get { "course": "CHEM", "discussion": "3001", "replies": true }
inbox.get      { "id": "700" }
inbox.list     { "scope": "unread" }
```

```sh
canvas discussion reply CHEM 3001 --text "..."
canvas inbox send --to 77 --subject "Lab partner" --text "..."
canvas inbox reply 700 --text "..."
canvas operation status <journal-id>
canvas operation reconcile <journal-id>
```

## Why it is this way

The owner narrowed the MCP catalog to reads on 2026-09-10. A public post in a
course and a message in a person's name are approved at the terminal where
that person is, not through a host's form. Say so plainly if the user asks
why you cannot just send it.
