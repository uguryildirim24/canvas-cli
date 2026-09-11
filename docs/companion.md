# The browser companion — pointer, install steps, and the Chrome checks

`canvas-cli` can read the Canvas page you already have open. It does that
with a Chrome extension that you attach to one tab, and a broker process
that Chrome starts on your machine. There is no cookie import, no token in
the browser, and no fetch proxy.

**The contract is now [`SPEC.md` §24](SPEC.md#24-companion-broker-presence).**
That section holds the manifest permissions and what the extension may not
do, the gesture and attachment lifecycle, the zone rules on both ends, the
account probe, the text release rules and their bounds, the native host and
the broker ownership lock, the `bridge-native@1` and `bridge-ipc@1` message
tables, the refusal reasons, the consumer trust boundary, the commands, the
side panel, notes, follow, panel approvals, the `context.*` tools and the
`/context` resource, and `here@1`, `note@1`, `follow@1`, and `bridge@1`.

The rest of the surface is elsewhere in the same document: the `bridge.*`
config keys and the broker paths are [§9](SPEC.md#9-config-and-paths), the
command classes are [§5](SPEC.md#5-command-surface), the refusal reasons
join the exit table in [§14](SPEC.md#14-errors-and-exit-codes), the
plan-decision events are [§22](SPEC.md#22-coordinator-events-watch-notify),
and the payload shapes are
[Appendix D](SPEC.md#appendix-d-json-result-payloads).

The choices this file used to list are in §24, and the open ones are SPEC
§19 items 30, 31, 32, 41, 42, 43, 44, and 45.

Reviews: [`reviews/code-M7-a.md`](reviews/code-M7-a.md),
[`reviews/code-M7-b.md`](reviews/code-M7-b.md).

Two things stay here, because no section of the SPEC carries them: the
install walkthrough, and the table of checks that need a real browser.

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

Two settings, both optional:

```
canvas config set bridge.extension_id abcdefghijklmnopabcdefghijklmnop
canvas config set bridge.pause_hidden_after 5m
```

## What has been run, and what has not

SPEC §24.16 is the full record. The short of it: the broker, the panel,
notes, follow, the decision path, and every forgery path are tested end to
end against the shipped `canvas bridge host`, and the extension's own logic
and its renderer are tested under Node against the shipped files. **Nothing
in this package has been run in a real Chrome on this machine**, and no
Windows pipe was ever created.

If any check below behaves differently, it is a bug in this package, not in
your setup: nothing here was observed.

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
| Two consumers | Run `canvas here` from two terminals under different profiles | Each reads only after its own attach; no MCP host can attach at all since 2026-09-10 (SPEC §19 items 48 and 50) |
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
