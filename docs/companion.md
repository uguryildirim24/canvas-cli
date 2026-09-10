# The browser companion

`canvas-cli` can read the Canvas page you already have open. It does that
with a Chrome extension that you attach to one tab, and a broker process
that Chrome starts on your machine. There is no cookie import, no token in
the browser, and no fetch proxy.

This document is the install procedure, the protocol, the zone rules, and —
at the end — exactly what was and was not run on this machine.

## What it is, in one picture

```
Chrome tab you attached          your machine
┌──────────────────────┐        ┌───────────────────────────────────┐
│ content script       │        │ canvas bridge host                │
│  · classifies zone   │        │  · one identity, one owner        │
│  · GET users/self    │ native │  · holds the §10 identity lock    │
│  · extracts on ask   │◄──────►│  · serves bridge-ipc@1 on a       │
└──────────────────────┘  msgs  │    0600 Unix socket               │
                                └──────────────┬────────────────────┘
                                               │ bridge-ipc@1
                                   ┌───────────┴───────────┐
                                   │ canvas here           │
                                   │ canvas mcp            │
                                   │  context.attach/here/ │
                                   │  detach               │
                                   └───────────────────────┘
```

Nothing is shared until you click. `activeTab` gives the extension one tab
for as long as that tab stays on the origin you were on; a cross-origin
navigation ends the grant and a new gesture is required.

## Install

1. Install the native messaging manifest:

   ```
   canvas bridge install --extension-id <ID>
   ```

   `--browser chrome|chromium|edge` picks the browser. Without an id, the
   command writes the manifest with the id already in your config, if there
   is one.

2. Open `chrome://extensions` and turn on **Developer mode**.

3. Choose **Load unpacked** and select the `extension/` directory that
   ships with `canvas-cli`.

4. Copy the extension id Chrome shows. If it differs from the one you
   installed, run `canvas bridge install --extension-id <ID>` again. The
   broker refuses any other extension.

5. Open your Canvas tab and click the `canvas-cli` toolbar button, or press
   `Alt+Shift+C`.

6. Check it:

   ```
   canvas bridge status
   canvas here --json
   ```

### The manifest template

`canvas bridge install` writes this file, at mode `0600`:

```json
{
  "name": "com.canvas_cli.bridge",
  "description": "canvas-cli browser companion broker",
  "path": "/absolute/path/to/canvas",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://<ID>/"]
}
```

It goes to the browser's native messaging host directory:

| Platform | Directory |
|---|---|
| macOS | `~/Library/Application Support/<vendor>/NativeMessagingHosts/` |
| Linux | `~/.config/<vendor>/NativeMessagingHosts/` |
| Windows | a registry key, which `canvas bridge install` does not write |

`path` must be absolute, and it names the `canvas` binary itself. Chrome
starts it as `canvas chrome-extension://<id>/`, and `canvas` recognizes an
extension origin in that position and runs `bridge host`.

## Configuration

| Key | Default | What it does |
|---|---|---|
| `bridge.extension_id` | none | The only extension the broker will serve. |
| `bridge.pause_hidden_after` | `10m` | How long a hidden tab keeps sharing. `<n>h`, `<n>m`, or `<n>s`. |

```
canvas config set bridge.extension_id abcdefghijklmnopabcdefghijklmnop
canvas config set bridge.pause_hidden_after 5m
```

## Permissions, and the one deviation

The manifest asks for exactly four permissions:

| Permission | Why |
|---|---|
| `activeTab` | One tab, granted by your gesture, revoked by a cross-origin navigation. |
| `nativeMessaging` | The pipe to `canvas bridge host`. |
| `scripting` | See below. |
| `sidePanel` | The panel is a page of this extension. It reaches no site. |

There is no `host_permissions`, no `content_scripts`, no `cookies`, no
`webRequest`, no `tabs`, no `storage`, and no `externally_connectable`.
`tests/companion.rs` asserts all of that on the shipped manifest.

**The deviations.** The design names `activeTab` and `nativeMessaging` only.
Two more are declared, and neither reaches a page.

`sidePanel` opens the extension's own surface. It grants no host access, no
tab access, and no way to read anything: the panel is a page served from the
extension package, and everything it displays arrives from the native host.
Without it there is no place to show a person a plan before they approve it,
and REPORT §3.4 is explicit that the surface must not be a Canvas DOM
overlay.

`scripting` is the older one.
Chrome's own documentation for `chrome.scripting.executeScript` states that
`activeTab` allows it *if the `"scripting"` permission is also declared*.
`scripting` grants no host access of its own — every injection still needs
`activeTab`'s per-gesture grant — so declaring it widens nothing, and
without it the companion cannot read the page at all. The alternative, a
declarative `content_scripts` entry, would inject into every Canvas page
whether or not you asked, which is worse.

**Where the account probe runs.** The fixed `GET <origin>/api/v1/users/self`
runs in the isolated-world content script, not in the service worker. In the
page's own origin it is same-origin and needs no host permission; in the
worker it would need one. The response body is reduced to
`{ user_id, observed_at }` before it leaves the content script. It is sent
with `redirect: "error"`, `credentials: "same-origin"` and
`Accept: application/json`: a redirect to a login page is a failure, not an
answer.

## Zones

The content script classifies the page **before** it reads any of it. The
route gives one classification and the page's own frames give another, and
the stricter of the two wins.

| Zone | What it is | What leaves the browser |
|---|---|---|
| `open` | An ordinary course page, assignment, discussion, or page | Route, sanitized URL, title; text only when asked |
| `graded` | A gradebook or submission view | The same. Grades are read from the API, never scraped |
| `assessment` | A quiz or a graded assessment | Nothing at all |
| `external` | An LTI or external-tool frame | Nothing at all |
| `unknown` | A frame this build does not recognize | Nothing at all |

For the three opaque zones the bundle carries no route ids, no URL, no
title, and no text, and `content_reason` says `zone_opaque`. Sharing also
pauses when the tab enters an assessment.

Both ends classify. The extension classifies from the document it can see;
the host classifies again from the sanitized URL it was sent, and takes the
stricter reading, so the extension cannot talk the host into a more
permissive zone. An opaque page sends no URL at all, so the host has no path
of its own to read: an already-opaque classification stands as it is, and
anything else reads `unknown`.

An unrecognized frame makes the whole page `unknown`. That is deliberate:
the safe reading of a frame nobody recognizes is that it might be an
assessment.

## What text release costs

Metadata is served from the broker's own memory. It triggers no probe and no
page read.

Text — the passage you selected and the visible editor excerpt — is released
only when a consumer asks for it (`canvas here --text`, or
`context.here` with `include_text: true`), and only after the extension
probes the account again and the broker verifies it against the identity.
An opaque zone releases none. Hidden inputs, credential-looking field names,
and capability-bearing query parameters (`verifier`, `Signature`,
`X-Amz-*`, `token`, `sig`, `Policy`, `Expires`) are stripped before
anything leaves the page. The total payload is bounded at 64 KiB of UTF-8,
cut on a character boundary, with `truncated: true` when it was cut.

## The protocols

### Extension ↔ host: `bridge-native@1`

Chrome's native messaging framing: a 4-byte **native-endian** length, then
UTF-8 JSON. A message over 1 MiB is refused from the length alone, before
any buffer is allocated for it.

| Extension → host | Host → extension |
|---|---|
| `hello` — protocol, extension id, browser-profile instance | `ready` — protocol, identity key, origin, `pause_hidden_after_ms` |
| `attach` — one observation | `attached` — the attachment state |
| `update` — a navigation, a visibility change, a re-probe | `request_text` — ask for the selection and excerpt |
| `text` — the answer, with a fresh account probe | `refused` — a named reason |
| `pause` / `detach` — with a cause | `detach` — sharing is over |
| `navigate_ack` — the companion took a navigation, or refused it | `navigate` — go to this URL, inside the granted origin |
| `navigate_outcome` — what became of it, later | `note` — one note for the panel to display |
| `decision` — the person approved, declined, or cancelled a plan | `panel` — everything the panel shows |
| `panel_hello` — the panel opened, or reloaded | |

### Host ↔ consumer: `bridge-ipc@1`

Newline-delimited JSON on `<data root>/bridge/<identity-key>.sock`, mode
`0600` inside a `0700` directory. A request line over 64 KiB is refused as it
arrives.

| Operation | Who may call it |
|---|---|
| `attachments.list` | Anyone. It carries no attachment id and no page content. |
| `attach` | A consumer, naming itself. It receives the attachment id. |
| `here` | A consumer that attached, or the CLI when there is one attachment. The caller is checked before the browser is asked for anything. |
| `detach` | A named consumer gives up its own share; the CLI ends the attachment. |
| `release` | `identity remove`, before it takes the exclusive lock. |
| `note` | A consumer that attached, or the CLI. It holds one note for display and writes nothing. |
| `follow` | The same callers. It asks the browser to move the tab inside the granted origin. |

**There is no approval operation on this socket, and there will not be one.**
A plan is approved over the native-messaging path, from the panel, and
nowhere else. Anything that reaches this socket has, by construction, nothing
to say about a plan.

On Windows the endpoint is the named pipe
`\\.\pipe\canvas-cli-<identity-key>`, with a protected DACL that names only
the pipe's owner and `SY`.

### Ownership and identity

- `<data root>/bridge/<identity-key>.lock` is held exclusively for the
  host's lifetime. A second host for the same identity reports the first one
  and exits 8. The lock file itself is persistent and is removed only by
  `identity remove`.
- A stale socket is unlinked only by a process that already holds that lock,
  so a live endpoint is never removed by a newcomer.
- The host holds the SPEC §10 **shared** identity lock while it runs.
  `identity remove` asks it to let go first (`release`); the host detaches,
  tells the extension, and exits. If it does not let go within five seconds,
  `identity remove` reports the identity busy and changes nothing.
- The host re-reads `identity.json` every two seconds. If the identity is
  gone or was replaced, it stops.

## Reasons, and what they mean

`canvas here` and `context.here` name why a bundle is unavailable.

| Reason | Exit | What to do |
|---|---|---|
| `not_attached` | 8 | Open a Canvas tab and click the companion. |
| `paused` | 8 | Return to the tab. |
| `validating` | 8 | The page just changed; ask again in a moment. |
| `account_mismatch` | 8 | The browser is signed in as another Canvas account. Nothing was joined. |
| `bridge_unavailable` | 8 | No host is running. Run `canvas bridge status`. |
| `stale_generation` | 8 | The page moved on; call `here` again and work from the new bundle. |
| `note_too_large` | 8 | The note is over 8 KiB. Nothing was held. Shorten it. |
| `source_ref_rejected` | 8 | A source ref is not `canvas://` or `https` on the attached origin. Nothing was held. |
| `note_rejected` | 8 | The note was refused for another named bound. Nothing was held. |
| `origin_mismatch` | 8 | The follow target is not inside the granted origin. |
| `navigation_timeout` | 8 | The companion did not acknowledge in two seconds. The tab may still have moved. |
| `zone_opaque` | 0 | The attachment is healthy and this page carries nothing. Not a refusal. |

## The side panel

The panel is a page of the extension. It opens on the same gesture that
shares the tab, and it shows four things: what is attached, the plans waiting
for your decision, the notes an agent left you, and the submission journals
for the page you are on.

It has **no model and no chat backend**, it opens no database, and it makes
no request of its own — not to Canvas, not anywhere. Everything it draws
arrives from `canvas bridge host` as one `panel` message. The service worker
relays that message; it composes none of it.

The API side of the page travels as whole §7 envelopes, the same documents
`canvas course --json` and `canvas assignment --json` print, each keeping its
own freshness. The host reads them **offline**: a panel is redrawn whenever
the log moves or the person opens it, and a surface that fetched on every
redraw would make the browser the reason Canvas is called. So a row the cache
has not refreshed is shown as stale, and a fact the CLI does not have yet is
shown as not held — never borrowed from the browser observation beside it,
which is a hint and not a fact. There is no page text in any of this.

### Notes

`canvas note --text …`, or `context.note` from an agent, holds one note for
the attachment. A note is inert:

- it is not a Canvas write. Nothing about it reaches Canvas;
- it is not an approval. No note, whatever its text or its refs say, can
  approve, decline, or cancel a plan;
- it is not HTML. The text travels as Markdown source and the panel renders a
  small subset of it — headings, paragraphs, lists, quotes, code, bold,
  italic, links — as elements built one at a time with their text set as
  text. There is no HTML parser in that path, so a tag stays a tag on screen;
  no image is ever fetched; and a link survives only when it is `https` on
  the granted origin with no credentials in it. Everything else is shown
  struck through and marked *(link removed)*, so you see that something
  claimed to be a link rather than seeing a link that lies.

A note is at most 8 KiB and carries at most 16 source refs, each either
`canvas://…` or an `https` URL on the attached origin. Breaking either bound
refuses the whole note; nothing is truncated and nothing partial is shown.
Notes are held for the attachment's lifetime, survive a navigation and a
pause, and are erased on detach.

### Follow

`canvas open <target> --follow`, or `context.follow`, asks the attached tab
to go somewhere inside the granted origin. The target goes through the
ordinary `canvas open` resolver first, so a target outside this identity's
Canvas fails at exit 6 and never reaches the browser.

The answer is a **dispatch acknowledgement**: the companion took the request.
It is not a page load. What became of the page arrives later, and lands on
`here@1` as `browser.follow.load` — `loaded`, `failed`, or `unknown`.

Navigating is not one of this project's API previews. A preview promises to
change nothing. Handing a URL to a browser promises no such thing: Canvas'
own page controllers run, and the discussion controller marks a topic read
when it renders it. `follow@1` says so in `side_effects`, and `canvas open
--follow` says so on stderr.

### Approvals

When a plan is waiting for a decision, the panel shows the frozen plan — the
assignment, the files with their sizes and hashes, a bounded text preview,
the digests, the baseline attempt, and when the plan expires — and offers
approve, decline, and cancel.

The decision travels from the panel to the host over native messaging,
carrying the handle the panel was shown. The host then checks, in order and
before anything moves: that the handle is one it issued for that plan and is
still unspent, compared in constant time; that the digest the panel echoed is
the plan's; that the stored plan still describes itself; and that the plan
belongs to the current identity generation. Only then does it call
`plan::approve` with `channel = "panel"`, which checks the handle, the
consumer, and the expiry again inside its own transaction.

A page script cannot reach any of this. It runs in the extension's own
surface, and the socket a page might reach through a consumer has no approval
message at all.

Approval events are private. The log records `plan.approved`,
`plan.declined`, or `plan.cancelled` with the plan id and the decision, and
nothing else — no target, no digest, no bytes.

### The status feed

The panel is one more consumer of the M6-c event log and follows its cursor
rules. A position the log can no longer replay is not quietly restarted: the
panel says **refresh** and starts again.

Journal and receipt states are shown with their exact SPEC §12.2 names. The
sentence beside a name explains it and never replaces it. `matched` says the
attribution is unproven. `outcome_unknown` says the outcome was never
observed. Nothing is drawn as finished except `submitted`, which is the one
state in which this machine saw the response that created the attempt.

## Choices this package made

The design report is silent on several points. Each was decided the same
way: expose less, and never join API and browser data without a verified
account.

1. **`scripting` is declared.** Chrome requires it for programmatic
   injection even under `activeTab`. See the deviation above.
2. **The probe runs in the content script**, not the service worker, so it
   is same-origin and needs no host permission.
3. **An unrecognized frame makes the page `unknown`**, and an unknown zone
   exposes nothing. A frame nobody recognizes could be an assessment.
4. **`attachments.list` carries no attachment id and no page content.** The
   attachment id is the capability, so a listing must not hand it out. Status
   output therefore names the state, the origin, the account id, and the
   zone, and never the page.
5. **A consumer handle is set by the adapter, never by a model.**
   `context.here` from `canvas mcp` always carries the calling host's own
   handle, so one consumer cannot read another's attachment even with a
   stolen id.
6. **`context.detach` gives up only the caller's share.** Ending the
   attachment for everyone is `canvas bridge detach`, a human act.
7. **Reading the `/context/<handle>` resource attaches nobody, and reads
   nobody else.** Until that consumer calls `context.attach`, the resource
   answers `not_attached`, and a subscription never opts anybody in. The
   handle in the URI must be the reading session's own: naming another
   consumer's handle reads exactly what an unattached consumer reads, so the
   resource never reports whether that other handle attached at all.
8. **`context.attach` returns the handle and the state, never the page.**
   Opting in and reading are two decisions.
9. **Browser context is never cacheable.** `ttl_ms` is `0` and the bundle
   carries no `freshness` row of its own, so nothing downstream can serve a
   stale observation as a fact.
10. **`zone_opaque` is not a refusal.** The attachment is healthy; the
    answer is a bundle with no page content, and the exit is 0. Every other
    reason exits 8.
11. **The API and browser sides are separate documents.** `api` holds whole
    §7 envelopes, each with its own freshness. A browser extract never
    updates one of them.
12. **The socket path is length-checked.** A `sockaddr_un` holds about a
    hundred bytes. A data root deep enough to overrun it produces a named
    local failure that says to set `CANVAS_DATA_ROOT` somewhere shorter,
    rather than an opaque `bind` error.

The report is silent on more points in this round. Each was decided the same
way as before, under one reading: **nothing on a web page can approve
anything, and the panel never shows more certainty than the journal holds.**

13. **`bridge-ipc@1` has no approval operation.** A decision travels only
    over native messaging. The alternative — an approval op on the socket —
    would put the approval path within reach of anything that can talk to a
    consumer, and the whole point of the handle is that reaching it is hard.
14. **The panel is fed entirely by the host.** It opens no database, makes no
    request, and has no model behind it. A panel that could read for itself
    would be a second, unaudited path to the same data.
15. **A handle is compared in constant time**, and the panel's echo *selects*
    a stored row rather than supplying one. Echoing a digest is not proof of
    holding a handle, and the digest is checked separately anyway.
16. **An oversize note is refused, never truncated.** A note cut in half
    changes what it says, and the person cannot tell that it was cut.
17. **A source ref must be `canvas://` or `https` on the granted origin, with
    no username and no password.** `Url::origin()` ignores credentials, so a
    URL like `https://you@canvas.example/…` has the right origin and reads
    as another host to a person. The panel shows refs, so it refuses them.
18. **Notes and navigation are bound to the navigation generation, in both
    directions.** A generation behind the browser is stale; a generation
    ahead of it is `stale_generation` too, because an agent that names a
    generation the tab has not reached is working from something other than
    what it read.
19. **The CLI may omit `--generation`; the agent tools may not.** A person
    typing a note is looking at the tab. An agent works from a bundle it read
    earlier, which may describe a page the person has already left.
20. **Dispatch and load are two facts.** `follow@1` answers on the
    acknowledgement, and the load outcome arrives later on `here@1`. An
    outcome that names a request the bundle no longer reports changes
    nothing, so a late answer for a replaced navigation cannot mislead.
21. **A follow belongs to the consumer that asked for it.** One agent's
    navigation is not reported to another as its own.
22. **This build never reports `failed` for a load.** Seeing a load failure
    needs `webNavigation`, which would grant standing visibility of every
    navigation in the browser. Ten seconds after a navigation with nothing
    observed, the answer is `unknown`, which is what is actually known.
23. **Notes survive a navigation and a pause; they die with the
    attachment.** They are a message to the person, not an observation of a
    page.
24. **The panel's API side is read offline, and only offline.** The panel is
    redrawn on every log move; fetching on each redraw would make opening a
    browser panel the reason Canvas is called.
25. **An approval event carries ids and the decision only.** Invalidating a
    plan because a fact changed is not a decision and records no event.
26. **`MAX_REQUEST_BYTES` is larger than the note bound.** They used to be
    the same 8 KiB, which made a note at its own limit come back as
    `protocol`. A person can act on `note_too_large`; nobody can act on
    `protocol`.

## What was run, and what was not

This section is the honest record. Nothing below is a claim about a flow
that was not executed.

### Run, and passing

- **The broker end to end, against a real process.** `tests/bridge.rs`
  starts the shipped `canvas bridge host` the way Chrome starts it —
  `canvas chrome-extension://<id>/`, native-messaging framing on its pipes —
  and drives it with hand-written messages. It covers: the `ready`
  handshake; `hello` and `attach`; `canvas bridge status` and
  `canvas here --json` over the real socket; a wrong extension id, a
  non-extension origin, and no origin at all, each refused with exit 8; a
  stale socket cleared under ownership and a live endpoint never unlinked;
  a second host reporting the first; `identity remove` completing after the
  cooperative release, with the ownership lock gone and the root identity
  lock untouched; and an assertion over every byte that crossed either pipe
  that no token or cookie name appears on it, with a capability parameter
  and a planted secret put on a URL the companion reports, so the assertion
  has something to catch.
- **Two consumers, over a real `canvas mcp`.** Two server instances under
  different host names attach and read through `context.attach`,
  `context.here`, `context.detach` and the `canvas://…/context/<handle>`
  resource. Only the consumer that attached reads the bundle; a stolen
  attachment id does not serve the other one; reading the other consumer's
  resource by name does not either; and a consumer letting go leaves the tab
  attached for the person.
- **The companion's own logic, under Node.** `cd extension && npm test`
  runs the route classifier, the zone classifier, the sanitizer, and the
  byte bounds against fixture HTML with planted secrets. No dependency is
  installed to run it.
- **The broker's decisions, as unit tests.** `canvas-core::bridge` covers
  the framing bounds (an oversize length is rejected from the header alone,
  before any allocation), account mismatch, a failed or redirected probe,
  stale document and navigation generations, one identity holding one
  attachment, a second browser profile inheriting nothing, opaque zones
  exposing nothing, and the 64 KiB ceiling on character boundaries.
- **The panel, notes, follow, and the decision path, against a real
  process.** `tests/m7b.rs` drives the same shipped host. It covers: a note
  from an agent reaching the panel with no model behind it; an oversize note
  and an off-origin, plaintext-`http`, `javascript:`, credentialed, or empty
  `canvas://` ref each refused whole, with nothing held; notes and follows
  bound to the navigation generation in both directions; a stale follow and a
  cross-origin follow that never reach the browser; a follow acknowledgement
  separated from its load outcome, with the outcome landing on the bundle
  and a late outcome for a replaced navigation changing nothing; approve,
  decline, and cancel through the panel path, each leaving the recorded
  event and, for the two invalidations, the reason that tells them apart; and
  the forgery paths below.
- **Every forgery path, explicitly.** A note whose text and refs are an
  approval payload is held for display and moves nothing. The socket is asked
  to approve and answers `refused: protocol`, because `bridge-ipc@1` names no
  such operation. The native path is given a guessed handle, the plan digest
  used as the handle, a rewritten digest, another plan's id, and a fourth
  decision word; each is refused and the plan stays `prepared`. The real
  decision is then made and works, so those refusals were the checks and not
  a broken path. Replaying a spent handle as a decline changes nothing.
- **The panel's renderer and sanitizer, under Node.** `cd extension && npm
  test` renders fixture notes — an ordinary one and a hostile one carrying
  `<script>`, an `onerror` image, a `javascript:` link, an off-origin link, a
  credentialed link, a raw anchor, and a line of text shaped like an approval
  — and asserts that no node of the result is a link, an image, or a control,
  that the tags are on screen as text, and that oversize text is bounded
  before layout. It also asserts that only `submitted` is drawn as done, that
  `matched` says the attribution is unproven, and that `outcome_unknown` says
  so.
- **The panel surface builds no markup from a string.**
  `tests/companion.rs` reads the shipped panel files and fails on
  `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`, `eval(`,
  and `new Function`, and checks that every script the panel page loads
  ships.
- **The benchmark.** `cargo xtask bench --bridge` starts a real host and
  times a warm metadata `here` over the socket and a follow acknowledgement.
  The numbers are in `docs/bench.md`.

### Not run

- **Real Chrome, on this machine.** The companion was not loaded into a
  browser here, and no gesture, navigation, account switch, or two-tab case
  was exercised in a real Chrome. Two things stood in the way, and neither
  was worked around: installing the native messaging manifest writes into
  the user's own Chrome profile directory, and the probe and the API side
  both need a real Canvas account, which this environment does not have.
  Every Chrome-facing rule in this package therefore rests on Chrome's
  documentation plus the tests above, not on observation.

  So, explicitly: **the following are untested and unverified here** — load
  unpacked, the toolbar gesture, a same-origin navigation keeping the
  `activeTab` grant, a cross-origin navigation revoking it, a Canvas account
  switch producing `account_mismatch`, two tabs, and a broker restart driven
  by Chrome rather than by a test.

  This round adds to that list, and none of it was run in a real Chrome
  either: **the side panel opening on the gesture**; a note rendered on
  screen; hostile markup proving inert in a real document rather than in the
  node tree the tests read; `chrome.tabs.update` actually moving a tab; the
  `loaded` outcome arriving from a real navigation; a stale follow refused
  with a real tab behind it; the approve, decline, and cancel buttons; and a
  forged approval rejected with a real page in the tab. The checks below say
  how to run each of them.

  What *is* established without Chrome: everything the host decides, which
  is where every check that matters lives. The renderer and the view model
  are exercised as the browser runs them — `npm test` imports the shipped
  files, not a copy — but nothing puts their output into a real document
  here.

- **Windows.** The named pipe and its SDDL descriptor are compiled and unit
  tested on every platform, but no pipe was created and no Windows registry
  key was written. `canvas bridge install` reports Windows as unsupported
  rather than guessing at the registry.

### How to run the Chrome checks yourself

Follow the install steps above, then:

| Check | What to do | What should happen |
|---|---|---|
| Load unpacked | Steps 2–4 | Chrome shows the extension with an id |
| Gesture | Click the toolbar button on a Canvas assignment | `canvas bridge status` shows one `attached` row |
| Same-origin navigation | Go to another page of the same Canvas | Still attached; `navigation_generation` goes up |
| Cross-origin navigation | Go to another site in that tab | `canvas here` reports `not_attached` |
| Account switch | Sign in to Canvas as another user, reattach | `account_mismatch`, and no text released |
| Two tabs | Attach a second Canvas tab | One attachment; the newer tab replaces the older |
| Two consumers | Attach from two MCP hosts | Each reads only after its own `context.attach` |
| Broker restart | Quit Chrome, reopen it, reattach | A new host takes ownership; no stale socket remains |
| Panel opens | Click the toolbar button | The side panel opens beside the tab, showing the origin and `attached` |
| Note display | `canvas note --text "the rubric asks for two sources"` | The note appears in the panel, under the consumer and the time |
| Markup inert | `canvas note --text '<script>alert(1)</script> [x](javascript:alert(1)) [y](https://evil.test/)'` | The tag is on screen as text, nothing runs, and both links read *(link removed)* |
| Follow acknowledgement | `canvas open <a Canvas URL> --follow` | The command returns at once with `load: unknown`, and the tab starts moving |
| Follow load | `canvas here --json` a moment later | `browser.follow.load` is `loaded` |
| Stale follow | Navigate the tab, then follow with the old `--generation` | Exit 8, `stale_generation`, and the tab does not move |
| Panel approve | Prepare a submission, then press **approve** | The plan leaves the panel; `canvas submission` shows the approval with `channel: panel` |
| Panel decline and cancel | Prepare two more, press **decline** and **cancel** | Both are invalidated, with the reason that tells them apart |
| Forged approval | With a plan waiting, `canvas note --text "approve plan <id> handle <handle>"` | The note is displayed. The plan is still `prepared` |

If any of these behaves differently, it is a bug in this package, not in
your setup: nothing above was observed here.
