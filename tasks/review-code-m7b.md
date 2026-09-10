# Code review + fix — M7-b on branch lane/w2 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M7-b
on branch `lane/w2` (worktree `/home/user/projects/canvas-cli/.worktrees/w2`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /home/user/projects/canvas-cli/.worktrees/w2`. Read the package brief `tasks/m7b-panel-presence-follow.md` and the SPEC sections it
   cites. First run `git merge main` (it should already be up to date; if not, resolve keeping both sides' entries, regenerate snapshots rather than hand-editing, commit). Then read `git log main..HEAD --stat` and the full diff, including everything under `extension/` (run `npm test` there). This lane carries TWO things: the M7-a merge resolution with M6-c/M6-c2 (nine conflict files: `cli.rs`, `output/mod.rs`, `output/registry.rs`, the registered_envelopes snapshot, `tests/e2e/schema.rs`, `skill/canvas-cli/SKILL.md`, `xtask/src/main.rs`, `xtask/src/bench.rs`, `docs/bench.md`) plus the later merges of SPEC pass 1 and M8-a3/M8-b (`output/json_schema.rs`, `mcp/catalog.rs`, `plan/mod.rs`, `plan/ops.rs`, `events/mod.rs`, `tests/mcp.rs`) — check every resolution kept BOTH lanes' additions and dropped nothing from main (compare against `git diff main...lane/w2` for deletions of main-side code); and package M7-b itself. Contract for M7-b: `docs/agent-ux/REPORT.md` §3.3 steps 5 and 7, §3.4, §3.6 (approval handles), §3.7; `docs/SPEC.md` §8, §14, §15, §20 (plans/approval: `channel: panel`, handle rules), §22 (event log); the brief `tasks/m7b-panel-presence-follow.md` and its acceptance column. Attack it as an attacker would: can ANY page content, note text, source ref, or socket message approve, decline, or cancel a plan (the only legitimate path is the panel's decision over native messaging carrying the stored handle)? Is the handle compared in constant time, bound to the handle's consumer, single-use, expiring, and is the echoed digest checked against the plan's own digest and the identity generation? Is a note rendered with `createElement`/`textContent` only — grep the shipped panel files for `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`, `eval`, `new Function`, `srcdoc`, `javascript:` handling — and is a hostile note (script tags, `<img onerror>`, `javascript:` and `data:` links, credentialed `https://user@host` refs, off-origin refs, an 8 KiB+1 body) refused or rendered inert with no image and no live link? Are notes and follows generation-bound in BOTH directions and is a stale follow exit 8 with nothing sent to the browser? Is a cross-origin `--follow` target refused at exit 6 before the browser is asked? Does `follow@1` report the acknowledgement only and does the load outcome land on `here@1` without ever claiming `failed`? Does the panel show only whole §7 envelopes with their freshness, never page text, never a fact borrowed from the browser observation? Does the status feed follow the event log via the host's watchdog with no Canvas request, and does `resync_required` reach the panel? Does the `sidePanel` permission grant nothing beyond the extension's own surface (manifest still has no `host_permissions`, `tabs`, `webNavigation`, `storage`, `cookies`)? Are `plan.approved|declined|cancelled` events payload-minimal (ids and decision only)? Is `MAX_REQUEST_BYTES` 64 KiB enforced before allocation? Do the `note@1` and `follow@1` registry entries, fixtures, snapshots, README rows, skill sections, and MCP catalog allowlist all agree, and does the MCP-versus-CLI parity test still exclude only `.execute` and `context.*`? The worker ran nothing in real Chrome; check that `docs/companion.md` says so plainly and that no claim in the docs, the skill, or the README exceeds what the tests prove. You may not run real Chrome against the owner's Canvas either: do not install the native-messaging manifest into the owner's Chrome profile, do not load the extension into the owner's browser, and do not touch the owner's identity or credentials; test only against the fixtures and a scratch data root. Rerun `cargo xtask bench --runs 3` and `--bridge --runs 3`. The worker's final report, for reference:

```
# M7-b — Presence, panel approvals, follow (worker w2, branch `lane/w2`)

Worktree `/home/user/projects/canvas-cli/.worktrees/w2`,
`CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/w2`.
Nothing pushed. Nothing merged into `main`. Working tree clean; `lane/w2` is
up to date with `main`.

---

## Task 1 — the M7-a merge into main (`28f1f3b`)

`git merge main` conflicted with M6-c and M6-c2 in nine places. Every one was
resolved keeping **both** lanes' additions.

| File | Resolution |
|---|---|
| `crates/canvas-cli/src/cli.rs` | After-help lists `bridge install\|host\|status\|detach, here` **and** `sync, watch, notify, cache stats\|clear\|path`. `has_raw_output()` keeps both the `Bridge{command: BridgeCommand::Host{..}}` arm and the `Notify{..}` arm. |
| `crates/canvas-cli/src/output/mod.rs` | `pub use registry::{…}` is the union of both lanes. |
| `crates/canvas-cli/src/output/registry.rs` | Entries ordered `SCHEMA_WATCH`, `SCHEMA_EVENT`, `SCHEMA_HERE`, then the three `SCHEMA_BRIDGE` variants (`status`, `install`, `detach`). |
| `crates/canvas-cli/src/output/snapshots/…registered_envelopes.snap` | Regenerated with `INSTA_FORCE_UPDATE=1`, not hand-edited. |
| `crates/canvas-cli/tests/e2e/schema.rs` | The SHAPES list follows the same order. |
| `skill/canvas-cli/SKILL.md` | Kept both the `context/` resource paragraph and the `subscriptions/listen` paragraph. |
| `xtask/src/main.rs` | Passes both `bridge` and `watch` into `bench::Options`. |
| `xtask/src/bench.rs` | `Options` keeps `bridge` and `watch`; `measure_bridge` alongside `measure_all(runs, watch)`; the generated line carries `--mcp`, `--bridge` and `--watch`; `document(...)` takes six arguments. `#[allow(clippy::struct_excessive_bools)]` added with a doc comment, matching the `cli.rs`/`main.rs` precedent. |
| `docs/bench.md` | Regenerated by `cargo xtask bench`, not hand-edited. |

Gates after the merge: `cargo fmt --check`, `cargo clippy -D warnings`,
`cargo nextest run` (821 pass), `cargo deny check`, `npm test` (33 pass),
`cargo +1.88 check` — all green.

---

## Task 2 — M7-b, what was built

Eleven commits on `lane/w2`, newest first:

```
a08011b merge main: M8-a3 operations, and the panel adapted to them
a6b0696 feat(bridge): the panel shows the page's API facts, with their freshness
3110ad8 merge main: SPEC v0.9 consolidation pass 1, M8-a3, and the round-10 briefs
e6f0cf1 feat(bridge): the panel's status feed follows the event log
cd74f97 docs(companion): the panel, notes, follow, the approval flow, and the record
38b0dbf feat(bench): measure the follow acknowledgement
15c29dd test(m7b): the acceptance tests for notes, follow, and the panel decision
5721914 feat(extension): the side panel, the note renderer, and the follow path
92b4b07 feat(bridge): the side panel's state, the person's decision, notes and follow
daa6d40 feat(plan): record a person's decision, and name the plans awaiting one
62237b7 feat(bridge): the note and follow protocol, and what the panel is shown
```

### 1. The side panel (deliverable 1)

New: `extension/src/panel.html`, `panel.js`, `panel_view.js`, `markdown.js`.
Manifest gains the `sidePanel` permission and a `side_panel` entry.
`background.js` opens the panel inside the same gesture that shares the tab
(before its first `await`, because opening needs the gesture), relays `note`,
`panel` and `navigate` from the host, and relays `panel_hello`,
`decision`, `navigate_ack` and `navigate_outcome` back. The worker composes
nothing and checks nothing.

The panel shows: the attachment state and consumers; the page's API facts as
whole §7 envelopes, each with its own freshness; the plans awaiting a
decision; the notes feed; the follow status; and the submission journals for
the page. It has no model, no chat backend, no database, and makes no request
of its own.

`tests/companion.rs` reads the shipped panel files and fails on `innerHTML`,
`outerHTML`, `insertAdjacentHTML`, `document.write`, `eval(` and
`new Function`, and checks that every script the panel page loads ships.

### 2. `context.note` / `canvas note` (deliverable 2)

- `crates/canvas-core/src/bridge/note.rs`: `MAX_NOTE_BYTES = 8 KiB`,
  `MAX_SOURCE_REFS = 16`, `is_allowed_ref`, `check`.
- `Op::Note` / `Body::Noted{note, attachment_id, held}` on `bridge-ipc@1`;
  `HostMessage::Note` on `bridge-native@1`.
- `crates/canvas-cli/src/commands/note.rs`, registry entry `note@1` with
  fixture, MCP tool `context.note`.
- The renderer (`extension/src/markdown.js`) produces a tree of plain objects
  — headings, paragraphs, lists, quotes, code, bold, italic, links — and the
  panel builds each node with `createElement` and `textContent`. There is no
  HTML parser in the path, no image is ever emitted, and a link survives only
  when it is `https` on the granted origin with no credentials. Everything
  else is shown struck through and marked *(link removed)*.

### 3. `context.follow` / `canvas open --follow` (deliverable 3)

- `Op::Follow` / `Body::Followed`; `HostMessage::Navigate`;
  `ExtensionMessage::NavigateAck` and `NavigateOutcome`.
- `commands/open.rs` factors out `resolve()` and adds `follow()`. The target
  goes through the ordinary `open` resolver first, so a cross-origin target
  fails at exit 6 and never reaches the browser.
- `follow@1` carries `side_effects` naming the read-state change, and the
  human path prints it on stderr.
- Load outcome arrives later on `here@1`'s `browser.follow.load`.

### 4. Panel approvals (deliverable 4)

- `crates/canvas-cli/src/bridge/panel.rs`: `apply_decision` reads the awaiting
  row from `plan::awaiting_decision` (so the panel's echo *selects* a row
  rather than supplying one), compares the handle in constant time, checks the
  echoed digest, the plan's self-digest and the identity generation, then
  calls `plan::approve(..., ApprovalChannel::Panel, entry.consumer, now)` /
  `decline` / `cancel`.
- `plan/ops.rs`: `awaiting_decision(store, now) -> Vec<Awaiting>` selecting
  `approval_handles JOIN plans` where the handle is unspent, the plan is
  `prepared`, and the handle has not expired. `Awaiting` carries the
  **handle's** consumer, which is the one `approve` validates.
- `events/kind.rs`: `PlanApproved`, `PlanDeclined`, `PlanCancelled` →
  `plan.approved|declined|cancelled`, group `"plan"`. `events/log.rs`:
  `insert_decision`. The payload is the plan id and the decision, nothing else.

### 5. Status feed (deliverable 5)

The host keeps the cursor the panel was last shown and, on the watchdog it
already runs every two seconds, notices when the log has moved past it and
pushes. A `panel_hello` restarts the feed at the beginning of what the log
still holds; a position the log cannot replay comes back as
`resync_required`, which the panel shows as **refresh**. Nothing polls Canvas.

### 6. Docs and measurements (deliverable 6)

- `docs/companion.md` extended: the `sidePanel` permission and its deviation
  note; the new native and IPC messages; the new reasons; a "The side panel"
  section covering notes, follow, approvals and the status feed; thirteen more
  named choices; and the honest record of what was run and what was not.
- `xtask/src/bench_bridge.rs` measures the follow acknowledgement (target
  p95 < 300 ms) alongside the warm `here`. The extension side answers from its
  own thread for that part.
- `docs/bench.md` regenerated by the tool.
- Registry `note@1` and `follow@1` with fixtures and snapshots; MCP
  `context.note` and `context.follow` in the catalog allowlist test; README
  rows; skill sections.

### 7. Tests (deliverable 7)

`crates/canvas-cli/tests/support/mod.rs` extracts the M7-a harness so
`tests/bridge.rs` and the new `tests/m7b.rs` drive the same fixture rather
than a copy of it. It gains a raw `socket()` helper, so a test can send what
the CLI would never send and watch the broker refuse it.

`tests/m7b.rs` (9 tests):

- a note from an agent reaches the panel with no model behind it;
- an oversize note and an off-origin, plaintext-`http`, `javascript:`,
  credentialed, or empty `canvas://` ref are each refused whole, nothing held;
- notes and follows are generation-bound both ways (behind and ahead);
- a follow is acknowledged before it is loaded, the load outcome arrives on a
  separate message and lands on the bundle, and a late outcome for a replaced
  navigation changes nothing;
- a stale or cross-origin follow never reaches the browser;
- approve, decline and cancel through the panel path, each leaving its event
  and, for the two invalidations, the reason that tells them apart;
- **every forgery path**: a note whose text and refs are an approval payload;
  the socket asked to approve (`refused: protocol`); a guessed handle; the
  plan digest used as the handle; a rewritten digest; another plan's id; a
  fourth decision word. Each is refused, the plan stays `prepared`, and the
  real decision then works — so the refusals were the checks and not a broken
  path. A spent handle replayed as a decline changes nothing;
- the panel carries whole API envelopes with their freshness, and no page text;
- the panel follows the event log without polling Canvas.

`extension/test/markdown.test.js` and `panel.test.js` with fixture notes
(`note-ordinary.md`, `note-hostile.md`): the hostile note yields no link, no
image and no control node; the tags are on screen as text; oversize text is
bounded before layout; link policy is judged by protocol, host and
credentials; only `submitted` is drawn as done; `matched` says the attribution
is unproven; `outcome_unknown` says so; an observed receipt and an unknown
outcome are distinct rows; a state the panel does not know is never called
done; a row that names no assignment is not called one.

---

## Choices made where the brief or the report is silent

Every one was decided under one reading: **nothing on a web page can approve
anything, and the panel never shows more certainty than the journal holds.**
All of them are also named in `docs/companion.md`.

1. **`bridge-ipc@1` has no approval operation, and will not get one.** A
   decision travels only over native messaging. An approval op on the socket
   would put the approval path within reach of anything that can talk to a
   consumer.
2. **The panel is fed entirely by the host.** It opens no database, makes no
   request, has no model. A panel that could read for itself would be a
   second, unaudited path to the same data.
3. **A handle is compared in constant time**, and the panel's echo *selects* a
   stored row rather than supplying one. Echoing a digest is not proof of
   holding a handle; the digest is checked separately anyway.
4. **An oversize note is refused, never truncated.** A note cut in half
   changes what it says and the person cannot tell it was cut.
5. **A source ref must be `canvas://` or `https` on the granted origin, with
   no username and no password.** `Url::origin()` ignores credentials, so
   `https://you@canvas.example/…` has the right origin and reads as another
   host to a person. The panel displays refs, so it refuses them.
6. **Notes and navigation are generation-bound in both directions.** Behind
   the browser is stale; *ahead* of it is `stale_generation` too, because an
   agent naming a generation the tab has not reached is working from something
   other than what it read.
7. **The CLI may omit `--generation`; the agent tools may not.** A person
   typing a note is looking at the tab. An agent works from a bundle it read
   earlier, which may describe a page the person has already left.
8. **Dispatch and load are two facts.** `follow@1` answers on the
   acknowledgement; the load outcome arrives later on `here@1`. An outcome
   naming a request the bundle no longer reports changes nothing.
9. **A follow belongs to the consumer that asked for it.** One agent's
   navigation is never reported to another as its own.
10. **This build never reports `failed` for a load.** Seeing a load failure
    needs `webNavigation`, which would grant standing visibility of every
    navigation in the browser. Ten seconds after a navigation with nothing
    observed, the answer is `unknown` — which is what is actually known.
11. **Notes survive a navigation and a pause; they die with the attachment.**
    They are a message to the person, not an observation of a page.
12. **An approval event carries ids and the decision only.** Invalidating a
    plan because a fact changed is not a decision and records no event.
13. **The panel's API side is read offline, and only offline.** The panel is
    redrawn on every log move; fetching on each redraw would make opening a
    browser panel the reason Canvas is called. A stale row is shown as stale;
    a fact the CLI does not hold is shown as not held, never borrowed from the
    browser observation beside it.
14. **`MAX_REQUEST_BYTES` rises from 8 KiB to 64 KiB.** It used to equal the
    note bound, so a note at its own limit came back as `protocol`. A person
    can act on `note_too_large`; nobody can act on `protocol`.
15. **`sidePanel` is declared** (a second manifest deviation beyond
    `scripting`). It opens the extension's own surface, grants no host or tab
    access, and REPORT §3.4 forbids a Canvas DOM overlay.

---

## Merges into the lane during M7-b

**`3110ad8` — SPEC v0.9 consolidation pass 1, M8-a3, round-10 briefs.** Two
conflicts. `output/json_schema.rs`: the import list is the union of M7-a's
`here@1`, M6-c's `event@1`/`watch@1` and M7-b's `note@1`/`follow@1`, and the
two new typed results joined the schema map. `docs/bench.md` regenerated.

**`a08011b` — M8-a3 operations.** Seven conflicts, all unions:
`output/mod.rs`, `mcp/catalog.rs`, `output/json_schema.rs`, `plan/mod.rs`,
`events/mod.rs`, `plan/ops.rs`, `tests/mcp.rs`. The schema map took main's
`(id, variant)` form and gained the three M7-b arms. The MCP-versus-CLI parity
test keeps both exclusions: every `.execute` (needs a recorded approval) and
every `context.*` (they name the calling consumer; the CLI does not).

M8-a3 generalised a journal beyond one assignment, so `PanelJournal.course_id`
and `assignment_id` became optional and a row naming no assignment is labelled
by its kind rather than as "assignment ?". The three new registry entries
carry main's new `command` field; `follow@1` names no command, because
`open --follow` is a flag on `open`.

`bench_bridge` also gained a deterministic shutdown in this round: the thread
answering navigations blocks on the host's pipe, so a stop flag alone could
leave it there. The flag is now set and one more follow is asked for, which
the thread answers before it sees the flag and returns.

---

## Gate results (final, after the last merge)

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo nextest run --all-features` | **920 passed, 0 failed, 0 skipped** |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | clean |
| `(cd extension && npm test)` | **50 passed, 0 failed** |
| `cargo xtask bench --runs 3` | all targets met |
| `cargo xtask bench --bridge --runs 3` | all targets met |
| `git merge main` | already up to date |
| `git status --short` | clean |

The final `docs/bench.md` was regenerated with
`cargo xtask bench --mcp --watch --bridge --runs 3`, so every lane's section
is in the one file rather than the last flag set overwriting the others.

Companion numbers in that file:

| Metric | p50 ms | p95 ms | Target p95 | Verdict |
|---|---:|---:|---:|---|
| warm metadata `here` over the socket | 0.034 | 0.034 | 100 | ok |
| follow acknowledgement | 1.178 | 1.178 | 300 | ok |

The follow number is a **dispatch acknowledgement**, not a page load. Nothing
in the measurement waits for a browser to render.

---

## What is untested in a real Chrome

**Nothing in this round was run in a real Chrome.** The companion was not
loaded into a browser here. Two things stood in the way and neither was worked
around: installing the native-messaging manifest writes into the user's own
Chrome profile directory, and the probe and the API side both need a real
Canvas account, which this environment does not have.

Explicitly untested and unverified here, added to the M7-a list:

- the side panel opening on the gesture;
- a note rendered on screen;
- hostile markup proving inert in a real document rather than in the node tree
  the tests read;
- `chrome.tabs.update` actually moving a tab;
- the `loaded` outcome arriving from a real navigation;
- a stale follow refused with a real tab behind it;
- the approve, decline and cancel buttons;
- a forged approval rejected with a real page in the tab.

Still untested from M7-a: load unpacked, the toolbar gesture, a same-origin
navigation keeping the `activeTab` grant, a cross-origin navigation revoking
it, an account switch producing `account_mismatch`, two tabs, and a broker
restart driven by Chrome rather than by a test. Windows also remains
unexercised: the named pipe and its SDDL descriptor compile and unit-test
everywhere, but no pipe was created and no registry key written.

What **is** established without Chrome: everything the host decides, which is
where every check that matters lives. The renderer and the view model are
exercised as the browser runs them — `npm test` imports the shipped files, not
a copy — but nothing puts their output into a real document here.

`docs/companion.md` carries a table with a check for each of the items above,
so a person with a Canvas account can run them.

---

DONE M7-b
```
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/rev-m7b`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M7-b):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M7-b.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M7-b.md`
