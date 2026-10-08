# Code review :  M7-b (side panel, `context.note`, `context.follow`, panel approvals), branch `lane/w2`

Reviewer: Claude Opus 5 (high). Base: `d668de0` (merge of `main` into
`lane/w2`; docs only, no conflict). Package brief:
`tasks/m7b-panel-presence-follow.md`. Reviewer brief:
`tasks/review-code-m7b.md`. Contract: `docs/agent-ux/REPORT.md` §3.3 steps 5
and 7, §3.4, §3.6, §3.7; `docs/SPEC.md` §8, §14, §15, §20, §22. This lane also
carries the M7-a merge resolution with M6-c/M6-c2 (`28f1f3b`) and the later
merges of SPEC pass 1 and M8-a3/M8-b.

## Verdict

**MERGE.** The approval path is sound at the place that matters: `bridge-ipc@1`
carries no approval operation, `PanelState` :  the only structure that holds a
handle :  travels over native messaging and never over the socket, and a
decision is checked against a stored `awaiting_decision` row for handle,
digest, self-digest, identity generation and consumer before `plan::approve`
re-checks all of it inside its own transaction. No note, source ref, page
string or socket message reaches it. The note renderer emits objects rather
than markup and the panel builds every node with `createElement` and
`textContent`. Four `review(M7-b):` commits fix four real defects; the most
serious was an availability defect, not a containment one :  an unbounded note
list let one consumer grow the panel push past the 1 MiB native-message limit,
which the host treats as a lost pipe and answers by shutting the broker down
for everyone. All eight gates are green at 923 Rust tests and 52 npm tests,
both benches meet every target, and the docs are honest that nothing in this
package was run in a real Chrome. Three items are listed under "Needs a
decision"; none blocks the merge.

## Gate results

Run with `CARGO_TARGET_DIR=<checkout>`
at `60bd3ff` (after the fixes).

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass |
| `cargo nextest run --all-features` | pass :  923 tests run, 923 passed, 0 skipped |
| `cargo deny check` | pass :  advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | pass |
| `(cd extension && npm test)` | pass :  52 tests, 52 pass, 0 fail |
| `cargo xtask bench --runs 3` | pass :  every SPEC §13 target met |
| `cargo xtask bench --bridge --runs 3` | pass :  warm `here` p50 0.027 ms, p95 0.027 ms (target 100); follow acknowledgement p50 1.337 ms, p95 1.337 ms (target 300) |
| `git merge main` | clean; `tasks/review-code-m7b.md` only |
| `git status --short` | empty |

Either bench flag on its own rewrites `docs/bench.md` and drops the other
lanes' sections, so the file was regenerated once at the end with
`--mcp --watch --bridge --runs 3`, which keeps all four.

Nothing was installed into Rolf's Chrome profile, the extension was never
loaded into Rolf's browser, and no command touched Rolf's identity or
credentials. Every test and both benches run against `tempfile` data roots with
`CANVAS_BRIDGE_HOME` pointed at a scratch directory; `bridge install` was never
run without it.

## Defects found and fixed

| # | Sev | Where | What was wrong | What I changed | Commit |
|---|---|---|---|---|---|
| 1 | high | `crates/canvas-core/src/bridge/state.rs:644` (`Broker::note`) | `note` pushed onto `current.notes` with no bound on the count, and every `push_panel` serializes all of them into one `HostMessage::Panel`. `framing::write_message` refuses more than `MAX_MESSAGE_BYTES` (1 MiB) and `Host::send` treats a write it cannot make as a lost pipe, so it calls `shutdown()`. About 128 notes at the 8 KiB limit :  which one consumer writes in a loop over the socket, and which an agent acting on a hostile page's instructions would write without meaning to :  stopped the broker for the person and for every other consumer. The size bound was enforced per note; nothing bounded their sum. | `note::MAX_NOTES = 32` is checked in `note::check` beside the size bound, so the count and the size are refused in the same place. Their worst case is 256 KiB, a quarter of the frame, leaving the journals, plans and API envelopes their room. The note over the bound is refused as `note_rejected`, not swapped for the oldest, for the reason an oversize note is refused rather than cut: the person keeps every note they were shown and the agent is told. Two new tests :  `an_attachment_holds_only_so_many_notes` pins the bound against the frame size arithmetically, `an_attachment_stops_holding_notes_before_the_panel_push_grows_too_large` drives the broker to the bound and checks nothing already held was dropped. | `3a12e7f` |
| 2 | medium | `extension/src/markdown.js:106` (the quote branch) | `renderMarkdown` recursed once per `>` on a line, with no bound of the kind `MAX_INLINE_DEPTH` already placed on emphasis. A note of `">".repeat(6000)` :  well inside the 8 KiB a note may carry :  threw `RangeError: Maximum call stack size exceeded` inside `panelView`. `draw()` calls `root.replaceChildren()` before it composes, so the panel went blank and stayed blank on every redraw: the plans awaiting a decision, which are why this surface exists, were gone because of text an agent wrote. Reproduced against the shipped file before the fix. | Two changes, because a bound alone would only cover the failure I found. Block quotes stop nesting at `MAX_QUOTE_DEPTH = 6`, and past it the body is text, exactly as an emphasis run past its own bound is text :  nothing is dropped. And `panelView` renders each note inside a guard, so a note that throws for any other reason becomes one row saying so with its text shown unrendered, while the rest of the panel draws. New tests `a_block_quote_cannot_be_nested_until_it_recurses_away` (which fails on the old file) and `a note that cannot be rendered costs that row and not the panel`. | `d0d8cb9` |
| 3 | medium | `crates/canvas-cli/src/bridge/panel.rs:126` (`apply_decision`) | The awaiting row was selected with `find(|entry| entry.plan.plan_id == plan_id)` and the handle then compared against whatever that first row held. A plan can carry more than one live handle :  every `issue_handle` inserts another, and `submit.rs:257`, `submit.rs:749` and `operation.rs:849` each issue one whenever a consumer asks for a decision :  and `approvals()` draws a row per handle. Pressing approve on the second row sent a real handle the panel itself had shown and the host answered `bad_handle`: the person pressed the button, the plan stayed `prepared`, and the panel said nothing. A legitimate decision refused, not a forgery admitted. | The row is selected by the handle, among the rows for that plan. The echo still only selects a stored row and never supplies one, and the comparison is still `constant_time_eq`; what changes is that a handle this host issued now finds its own row. New test `a_second_live_handle_on_one_plan_is_a_decision_the_person_can_make`, which times out waiting for the approval on the old code and also pins that the approval is recorded against that handle's consumer and `channel: panel`. | `d5d2ecf` |
| 4 | low | `crates/canvas-cli/tests/companion.rs:124` | `the_panel_builds_no_markup_from_a_string` checked `panel.js`, `panel_view.js` and `markdown.js` against a fixed list, but `panel.html` loads five scripts: `shared.js` and `sanitize.js` ran in the panel document unscanned, and a script added later would have joined them silently. The forbidden lists also omitted `srcdoc`, and the manifest assertion omitted `webNavigation` :  both named in this package's contract. | The file list now comes from `panel.html` itself, the same place the test already reads to check that each script ships, with a floor of five so the scan cannot pass by finding nothing. `srcdoc` joins the sink list and `webNavigation` the manifest's. `javascript:` cannot be forbidden as a string :  it appears in this surface only as the comment naming what the link policy blocks :  so the policy's protocol test is pinned instead. | `4a710f6` |

## What I attacked, and what held

- **Can anything but the panel approve a plan?** No, on three independent
  grounds. `bridge-ipc@1` has no approval operation, so a socket client
  speaking it perfectly has nothing to say :  `parse_request` answers
  `refused: protocol`. `PanelState`, the only structure carrying a handle,
  is constructed in `panel::state` and reaches exactly one place,
  `HostMessage::Panel` at `host.rs:510`, which is native messaging; no `Body`
  variant on the socket carries it, so a consumer cannot read a handle off the
  wire. And a decision that does arrive is checked against a row read from
  `awaiting_decision` :  the echo selects, it never supplies :  for handle
  (constant time), echoed digest, the plan's own `digest() == plan_sha256`,
  and the identity generation, before `plan::approve` re-checks the handle's
  existence, plan binding, `used_at`, consumer and expiry inside one
  `Immediate` transaction. `no_forged_approval_moves_a_plan` walks seven
  forgery paths and then makes the real decision, so the refusals are the
  checks and not a broken path.
- **A hostile note.** `note-hostile.md` plus my own cases: script tags,
  `<img onerror>`, `javascript:` and `data:` links, `https://user@host` on the
  granted origin, off-origin refs, `http:` refs, an empty `canvas://`, and
  8 KiB + 1. Nothing in `markdown.js` emits a string that becomes markup :
  the output is plain objects and `panel.js` builds each node with
  `createElement` and `textContent` :  so a tag is a tag on screen. No image
  node is ever produced: `![alt](url)` keeps the alt text and drops the URL
  entirely. `linkPolicy` blocks on protocol, then on credentials, then
  compares `url.origin`, so `https://x.test.evil.test` is not `https://x.test`
  and a userinfo URL is refused even when its origin is right. The host
  refuses a bad source ref before the note is held at all. Defect 2 was the
  one thing this note could still do, and it is fixed.
- **Generation binding in both directions.** `check_generation` is equality,
  not `>=`, so a note or follow naming a generation ahead of the tab is
  `stale_generation` too :  an agent naming a generation the tab has not
  reached is working from something other than what it read. Checked before
  the bounds and before the browser hears anything.
- **A cross-origin follow.** `open::follow` runs the ordinary `open` resolver
  first, so a target outside this identity's Canvas fails at exit 6 with the
  same envelope `canvas open` produces and never reaches the host. Past that,
  `may_follow` checks the capability, the generation and the origin :  scheme,
  `Url::origin()`, and credentials :  before `HostMessage::Navigate` is sent,
  and `background.js` checks the origin a third time before
  `chrome.tabs.update`.
- **Dispatch versus load.** `Body::Followed` always carries
  `load: LoadOutcome::Unknown`; the outcome arrives later on `here@1` via
  `navigated()`, matched by `request_id`, so a late answer for a replaced
  navigation changes nothing. The shipped `background.js` sends only `loaded`
  and `unknown` :  `failed` needs `webNavigation`, which the manifest does not
  hold and the test now asserts it does not :  and after `NAVIGATE_SETTLE_MS`
  with nothing observed the answer is `unknown`, which is what is known.
- **What the panel shows.** `panel::api` forces `offline: true, fresh: false`
  and takes whole `here@1`-shaped envelopes with their own `freshness` rows;
  `apiView` reads a name and the freshness out and nothing else. No page text
  is in those envelopes to leak, and no browser observation updates one. The
  status feed rides the host's existing two-second watchdog against the event
  log with no Canvas request, and `check_cursor` returning `Resync` reaches
  the panel as `resync_required` and draws as "refresh".
- **Journal states.** `panel::journals` passes `row.state` through with no
  mapping, and `JOURNAL_STATES` sets `done: true` for `submitted` alone.
  `matched` says attribution is unproven, `outcome_unknown` says the outcome
  was never observed, and a state the panel does not know is never drawn as
  done.
- **The permission.** `sidePanel` opens a page of the extension's own package
  and grants nothing about any site. The manifest still has no
  `host_permissions`, `content_scripts`, `externally_connectable`, `tabs`,
  `webNavigation`, `storage` or `cookies`, and the panel page has no inline
  script, so MV3's default `script-src 'self'` applies unweakened.
- **Bounds before allocation.** `MAX_REQUEST_BYTES` is 64 KiB and
  `read_bounded_line` compares against it while filling, before the line
  buffer can grow past it; `framing::read_message` validates the 4-byte header
  before the body `Vec` exists. Raising the socket bound from 8 KiB to 64 KiB
  is right: it used to equal the note bound, so a note at its own limit came
  back as `protocol`, which nobody can act on, instead of `note_too_large`,
  which a person can.
- **Events.** `plan.approved|declined|cancelled` carry `entity_key = plan_id`
  and `{state}` before and after, and nothing else :  no target, no digest, no
  byte of the payload. `record_plan_decision` commits inside the same
  transaction as the state change, so a decision the log names is one the plan
  row already carries, and `invalidate` with no event is still the path
  `execute` takes when a fact changed, which is not a decision.
- **The M7-a merge resolution (`28f1f3b`) and the later merges.** I diffed
  `main...lane/w2` for deletions of main-side code. The nine M7-a conflict
  files are unions: `registry.rs` and `e2e/schema.rs` carry `SCHEMA_WATCH`,
  `SCHEMA_EVENT`, `SCHEMA_HERE` and the three `SCHEMA_BRIDGE` variants in one
  order, `cli.rs` keeps both the `Bridge{Host}` and `Notify` raw-output arms,
  `xtask` keeps `bridge`, `watch` and `mcp`, and both snapshot and
  `docs/bench.md` were regenerated rather than edited. The only main-side code
  actually removed anywhere is the `context/<handle>` `not_attached`
  placeholder in `mcp/resources.rs`, which M7-a replaced with the real
  implementation, and the `canvas open` README row, which gained `--follow`.
  The M8-a3 merge kept main's `(id, variant)` schema-map form and main's new
  `command` field on the three new registry entries; `follow@1` names no
  command because `open --follow` is a flag. The MCP-versus-CLI parity test at
  `tests/mcp.rs:1249` still excludes exactly `.execute` and `context.*`, with
  the reason for each written down.
- **Docs against tests.** `docs/companion.md` says plainly that the companion
  was not loaded into a browser here and that no gesture, note render,
  navigation, panel button or forged approval was seen in a real Chrome, lists
  each untested item, and carries a table telling a person with a Canvas
  account how to run every one. I found no claim in the docs, the skill or the
  README that exceeds what the tests prove.

## Needs a decision

1. **`background.js` does not check `sender` on `chrome.runtime.onMessage`.**
   The `decision` branch relays whatever it is given to the host. Today
   nothing but the panel can reach it :  the manifest has no
   `externally_connectable`, and `content.js` has no `window.addEventListener
   ("message")`, so page script cannot cross into the isolated world :  and a
   forged relay would still need the handle, which only the panel is shown
   and which the host re-checks. So this is defence in depth against a future
   content script that bridges page messages, not a live hole. The obvious
   patch is to gate `decision` and `panel_hello` on
   `sender.url === chrome.runtime.getURL("src/panel.html")`. I did **not**
   ship it: `background.js` is the one file no test in this repo loads, this
   package has never run in a real Chrome, and a wrong predicate would break
   the panel silently with nothing here to catch it. It wants one run in a
   browser, which Rolf's constraints put out of reach for this review.
2. **`approve` treats an unparseable handle expiry as not expired.**
   `handle_expires.parse::<Timestamp>().is_ok_and(|d| now >= d)` fails open:
   a row whose `expires_at` will not parse is accepted. No path writes such a
   row :  `issue_handle` copies `plan.expires_at`, which `prepare` formats :  so
   it is unreachable today, and it is M6-a code this package did not touch.
   Whether a bound that cannot be read should refuse rather than admit is a
   question for the plan layer's owner, not something to change under an M7-b
   review.
3. **`awaiting_decision` filters expiry as text, not as a timestamp.**
   `h.expires_at > ?1` compares RFC 3339 strings, and jiff prints fractional
   seconds only when there are any, so `…:00.5Z` sorts below `…:00Z`. The
   window this can misjudge is under one second, it only decides whether the
   panel draws a row, and `approve` parses the expiry properly before anything
   moves. Worth tidying when the plan layer next changes; not worth a schema
   or format decision here.

## Files I touched

`crates/canvas-core/src/bridge/note.rs`,
`crates/canvas-core/src/bridge/state.rs`,
`crates/canvas-cli/src/bridge/panel.rs`,
`crates/canvas-cli/tests/companion.rs`, `crates/canvas-cli/tests/m7b.rs`,
`extension/src/markdown.js`, `extension/src/panel_view.js`,
`extension/test/markdown.test.js`, `extension/test/panel.test.js`,
`docs/companion.md`, `docs/bench.md`, and this file. No file owned by another
lane was renamed or reordered.
