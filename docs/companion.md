# Browser companion

The Chrome extension attaches one Canvas tab to a local broker started through
native messaging. There is no cookie import and no fetch proxy. Browser cookies
stay in Chrome. The extension makes one fixed same-origin account probe.

[SPEC.md section 24](SPEC.md#24-companion-broker-presence) describes permissions,
zones, text bounds, messages, local trust boundaries, panel decisions, notes,
and navigation. MCP exposes no browser tools or context resources. Use
`canvas here`, `canvas note`, and `canvas open --follow` from the command line.

## Install from source

Build `canvas` and put it on `PATH`, as described in [README.md](../README.md).
Login to the same Canvas account that is open in Chrome.

1. Open `chrome://extensions` and enable Developer mode.
2. Choose Load unpacked and select this repository's `extension/` directory.
3. Copy the extension identifier shown by Chrome.
4. Run `canvas bridge install --extension-id YOUR_EXTENSION_ID` with that identifier.
   `--browser chrome|chromium|edge` selects the manifest location.
5. Open a Canvas tab and click the companion toolbar button, or press `Alt+Shift+C`.
6. Inspect the attachment:

```sh
canvas bridge status
canvas here --json
```

The broker refuses an extension identifier other than the configured one.
`canvas bridge install` writes to the browser profile's native-host directory.
It does not install or enable the extension itself.

## Evidence and known gap

Existing Rust checks target the real native-host process with hand-written
protocol frames. The current review could not complete the socket-dependent
checks because the permitted temporary paths are too long. Node.js checks
passed against extension logic and a fixture DOM. Neither establishes a real
Chrome interaction. This review did not load the extension or use an
authenticated Canvas account. Windows named-pipe behavior was not checked.
See [testing.md](testing.md) for the recorded results.

An earlier report describes a broker disconnect after navigation, with no live
host behind the socket. A service-worker lifetime issue was suggested, not
confirmed. The repository does not establish a fix. Treat the companion as
experimental until the real-browser checks below have been completed.

## Manual release checks

These are checks to perform, not claimed results:

| Check | Action | Expected result |
|---|---|---|
| Attachment | Click the toolbar button on an assignment | One attached row in `canvas bridge status`. |
| Same-origin navigation | Navigate within Canvas | Sharing remains valid with a new generation. |
| Cross-origin navigation | Leave Canvas in the attached tab | Sharing is revoked. |
| Account switch | Sign in as a different account and reattach | Account mismatch, no text released. |
| Two tabs | Attach a second Canvas tab | The newer attachment replaces the older one. |
| Worker lifecycle | Leave the extension idle, then navigate | A live broker and valid attachment, or an explicit unavailable state. |
| Restart | Quit Chrome, reopen it, and reattach | A new host acquires ownership without unlinking a live endpoint. |
| Panel | Attach and run `canvas note --text "Check the rubric"` | The panel shows inert text. |
| Opaque content | Open a quiz or an external tool | No question, assessment, or tool text is exposed. |
| Follow | Run `canvas open YOUR_CANVAS_URL --follow` | Dispatch acknowledgement, then a separate load outcome in `canvas here`. |
| Generation race | Navigate during a follow request | The broker refuses stale context rather than moving the wrong page. |
| Forged approval | Display an approval-shaped note while a plan is prepared | The note cannot approve the plan. |

Replace `YOUR_CANVAS_URL` with a URL on the active account's Canvas origin.
Run write-decision checks only with an authorized disposable assignment or
account. Approval can authorize a write but does not establish compliance with a
course's AI rules. Do not test submissions on real coursework merely to validate
installation.
