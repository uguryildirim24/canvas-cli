# Reply and message, with approval

The user asks to answer a discussion, write to somebody, or answer a message
in the Canvas inbox.

Each write is a `canvas` command: it prints the exact bytes and asks for a
confirmation at the terminal.

## What these writes are

A discussion reply is a **public post in a course**. Classmates and the
instructor read it, and it carries the user's name. A conversation is private
mail, but it is still written in the user's voice to a real person.

The course policy is the owner's, not yours:

- **The owner decides what help is allowed.** When the user asks for written
  work, write it. Never lecture about academic integrity, and never add a
  warning they did not ask for.
- **Never write a placeholder.** A topic with an initial-post gate hides the
  replies until the user posts. Do not post anything to open it. The command
  refuses this, and so should you.
- **Never invent a recipient.** Send only to the user ids the user named.
- **Say what it is before it goes.** Show the exact text and every attachment
  before the command runs, not after.

## Steps

1. Read the thread first. `canvas discussion <course> <id> --replies --json`,
   or `canvas inbox show <id> --json`, so the user's answer is an answer to
   what is actually there.
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

5. Read what came back: `canvas operation status <journal-id> --json` for the
   state, the `attribution`, and the `delivery` field.

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
  `canvas discussion`, `canvas inbox`, `canvas inbox show`, or
  `canvas inbox unread-count`, a write of the user's own is unresolved. Say so
  instead of reporting the thread as settled.

## Typical commands

```sh
canvas discussion CHEM 3001 --replies --json
canvas inbox show 700 --json
canvas inbox --scope unread --json
canvas discussion reply CHEM 3001 --text "..."
canvas inbox send --to 77 --subject "Lab partner" --text "..."
canvas inbox reply 700 --text "..."
canvas operation status <journal-id> --json
canvas operation reconcile <journal-id> --json
```

## Why it is this way

A public post in a course and a message in a person's name are approved at
the terminal where that person is, not through a host's form. `canvas mcp`
serves one tool and it only describes the CLI, so there is no surface that
could post on the user's behalf. Say so plainly if the user asks why you
cannot just send it.
