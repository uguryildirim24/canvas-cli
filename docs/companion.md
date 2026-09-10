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
| `bridge.pause_hidden_after` | `10m` | How long a hidden tab keeps sharing. |

```
canvas config set bridge.extension_id abcdefghijklmnopabcdefghijklmnop
canvas config set bridge.pause_hidden_after 5m
```

## Permissions, and the one deviation

The manifest asks for exactly three permissions:

| Permission | Why |
|---|---|
| `activeTab` | One tab, granted by your gesture, revoked by a cross-origin navigation. |
| `nativeMessaging` | The pipe to `canvas bridge host`. |
| `scripting` | See below. |

There is no `host_permissions`, no `content_scripts`, no `cookies`, no
`webRequest`, no `tabs`, no `storage`, and no `externally_connectable`.
`tests/companion.rs` asserts all of that on the shipped manifest.

**The deviation.** The design names `activeTab` and `nativeMessaging` only.
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

### Host ↔ consumer: `bridge-ipc@1`

Newline-delimited JSON on `<data root>/bridge/<identity-key>.sock`, mode
`0600` inside a `0700` directory. A request line over 8 KiB is refused as it
arrives.

| Operation | Who may call it |
|---|---|
| `attachments.list` | Anyone. It carries no attachment id and no page content. |
| `attach` | A consumer, naming itself. It receives the attachment id. |
| `here` | A consumer that attached, or the CLI when there is one attachment. |
| `detach` | A named consumer gives up its own share; the CLI ends the attachment. |
| `release` | `identity remove`, before it takes the exclusive lock. |

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
| `stale_generation` | 8 | The page moved on; ask again. |
| `zone_opaque` | 0 | The attachment is healthy and this page carries nothing. Not a refusal. |

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
7. **Reading the `/context/<handle>` resource attaches nobody.** Until that
   consumer calls `context.attach`, the resource answers `not_attached`, and
   a subscription never opts anybody in.
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
  that no token, cookie name, or planted secret appears on it.
- **Two consumers, over a real `canvas mcp`.** Two server instances under
  different host names attach and read through `context.attach`,
  `context.here`, `context.detach` and the `canvas://…/context/<handle>`
  resource. Only the consumer that attached reads the bundle; a stolen
  attachment id does not serve the other one; and a consumer letting go
  leaves the tab attached for the person.
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
- **The benchmark.** `cargo xtask bench --bridge` starts a real host and
  times a warm metadata `here` over the socket. The numbers are in
  `docs/bench.md`.

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

If any of these behaves differently, it is a bug in this package, not in
your setup: nothing above was observed here.
