# Code review + fix — M7-a on branch lane/w2 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M7-a
on branch `lane/w2` (worktree `/Users/rolfie/projects/canvas-cli/.worktrees/w2`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /Users/rolfie/projects/canvas-cli/.worktrees/w2`. Read the package brief `tasks/m7a-companion-broker.md` and the SPEC sections it
   cites. First run `git merge main` (resolve if needed, keep both sides' entries, regenerate snapshots rather than hand-editing, commit). Then read `git log main..HEAD --stat` and the full diff, including everything under `extension/` (run `npm test` there). Contract: `docs/agent-ux/REPORT.md` §3.1, §3.3 steps 1–4 and 6, §3.4 (all of it), §3.5 (zones), and the brief; `docs/SPEC.md` §8, §10 (identity lock and removal), §14, §15. This is the first package with browser-facing code, so attack it as an attacker would: can any message over the native-messaging pipe or the broker socket carry a cookie, the PAT, a capability URL parameter, a hidden input, or text from an `assessment`, `external`, or `unknown` zone (read the extension's classifier and extractor line by line, then the tests' wire assertions)? Is the zone classified **before** any text is read, and is text released only after a fresh account probe whose `id` equals the identity's user id? Can a wrong extension id, a wrong origin, a redirected or failed probe, a stale document or navigation generation, or a second broker for the same identity ever be accepted? Is the 1 MiB native-message bound enforced before allocation and the 64 KiB text bound cut on a character boundary? Can a socket client attach itself to another consumer's attachment or read a `/context` resource it did not attach? Is the ownership lock never deleted, the stale socket unlinked only under ownership, the live endpoint never unlinked, and `identity remove` honest (busy, or completes only after the cooperative release)? Does `canvas bridge install` write only the native host manifest (exact id in `allowed_origins`, absolute binary path) and nothing into a browser profile? Does the extension request only `activeTab` and `nativeMessaging`, no `host_permissions`? The worker ran nothing in real Chrome; check that `docs/companion.md` says so plainly and that no claim in the docs, the skill, or the README exceeds what the tests prove. You may not run real Chrome against the owner's Canvas either: do not install the manifest into the owner's Chrome profile and do not touch the owner's identity or credentials; test only against the fixtures and a scratch data root. Rerun `cargo xtask bench --runs 3` and `--bridge --runs 3`. The worker's final report, for reference:

```
# M7-a — Companion attachment and broker (lane w2)

Branch `lane/w2`, worktree `/Users/rolfie/projects/canvas-cli/.worktrees/w2`.
Never pushed, never merged into `main`. `git status --short` is empty.

## Commits

| SHA | Subject |
|---|---|
| `299e1ed` | feat(bridge): the companion broker core: framing, IPC, zones, attachments |
| `e5719b5` | feat(bridge): canvas bridge install\|host\|status\|detach and canvas here |
| `181f730` | feat(extension): the MV3 companion, its classifiers, and its tests |
| `cc0938a` | Merge branch 'main' into lane/w2 |
| `137e3e9` | feat(bridge): context tools, the /context resource, release, and packaging |
| `d483be7` | Merge branch 'main' into lane/w2 |
| `559c114` | chore(bench): record the M7-a numbers, companion included |

The second merge brought in lane w1's M8-a and M8-a2 (pages, syllabus,
discussions, inbox, and their MCP tools). Four files conflicted —
`cli.rs`, `mcp/catalog.rs`, `output/mod.rs`, `output/registry.rs` — and
every conflict was resolved by keeping both lanes' entries, in both
lanes' order, renaming and reordering nothing.

## What was delivered

### 1. `extension/` — the MV3 companion

Plain JavaScript, **no dependency at all**, `npm test` on Node's own test
runner.

- `manifest.json`: MV3, permissions `activeTab`, `nativeMessaging`,
  `scripting`. No `host_permissions`, no `content_scripts`, no `cookies`,
  no `webRequest`, no `tabs`, no `storage`, no `externally_connectable`.
  A toolbar action and an `Alt+Shift+C` command are the gesture.
- `src/shared.js`, `routes.js`, `zones.js`, `sanitize.js`, `extract.js`
  are pure and add to one `globalThis.canvasCli` namespace, so Chrome
  injects them as classic scripts through
  `chrome.scripting.executeScript({files})` and Node imports them as ES
  modules. One copy of each file serves both.
- `src/content.js` runs in the isolated world: it mints a document id,
  performs the fixed `GET <origin>/api/v1/users/self` with
  `redirect: "error"`, `credentials: "same-origin"`,
  `Accept: application/json`, reduces the body to
  `{ user_id, observed_at }`, classifies the zone, and only then reads
  anything.
- `src/background.js` is the service worker: gesture, injection, port to
  the native host, navigation generation, cross-origin revocation, hidden
  tab timer. It performs **no fetch**.
- `test/dom.js` is a ~150-line fixture HTML parser written for this
  package, so the extractor tests need no jsdom.
- `test/fixtures/*.html` carry planted secrets (`SECRET-CSRF-abcdef123456`,
  `SECRET-ENV-token`, `SECRET-VERIFIER`, `SECRET-LTI`, `hunter2`) so the
  sanitizer and extractor tests can assert none of them ever appears.

### 2. `canvas bridge host` — the broker

`crates/canvas-cli/src/bridge/{host,client,owner,manifest,release}.rs`
and `crates/canvas-core/src/bridge/{framing,wire,text,endpoint,ipc,state}.rs`.

Chrome starts the binary as `canvas chrome-extension://<id>/`. `main.rs`
rewrites that argv into `bridge host` before clap sees it, so the manifest
can name the absolute path of the `canvas` binary itself, as the brief
requires.

The whole trust boundary lives in `canvas-core::bridge` as pure,
testable decisions: no browser, no socket, no process.

### 3. `bridge-ipc@1` — the broker protocol

Newline-delimited JSON on `<data root>/bridge/<identity-key>.sock`, mode
`0600` inside a `0700` directory. A request line over 8 KiB is refused as
it arrives, not after it is buffered. On Windows the endpoint is
`\\.\pipe\canvas-cli-<identity-key>` with a protected DACL naming only
the owner and `SY`.

| Operation | Who may call it |
|---|---|
| `attachments.list` | Anyone. No attachment id, no page content. |
| `attach` | A consumer naming itself; it receives the attachment id. |
| `here` | A consumer that attached, or the CLI when there is one attachment. |
| `detach` | A named consumer gives up its own share; the CLI ends the attachment. |
| `release` | `identity remove`, before it takes the exclusive lock. |

### 4. `canvas bridge install|host|status|detach` (class B, `bridge@1`)

`install` writes the native-messaging manifest atomically at mode `0600`
and persists `bridge.extension_id`. Windows is reported as unsupported
rather than guessed at, because the registry is not a
`NativeMessagingHosts` directory.

### 5. `canvas here` (class C, `here@1` = `ContextBundle@1`)

`api` carries whole §7 envelopes from the shared command handlers, each
with its own freshness. `browser` carries one observation with the
verified account and `ttl_ms: 0`. A browser extract never updates an API
field, and the bundle carries no freshness row of its own.

### 6. MCP: `context.attach`, `context.here`, `context.detach`, and the resource

Added to the M6-b catalog with REPORT §3.2 annotations, to the catalog
allowlist test, to `tests/skill.rs`'s `CATALOG`, and to `SKILL.md` (a new
"Where the user is" section plus the resource line). The catalog is now
**33 tools**.

`canvas://<identity-key>/<generation>/context/<consumer-handle>` is wired
to the broker. It replaced the `not_attached()` placeholder.

### 7. Packaging and docs

- `dist-workspace.toml` `include` gained `extension`.
- `cargo xtask bench --bridge` (`xtask/src/bench_bridge.rs`) starts a real
  host and times a warm metadata `here` over a real socket.
- `docs/companion.md` — the only new docs file.
- `docs/bench.md` — regenerated with both sections.

### 8. Tests

One explicit test per M7-a acceptance row. See the table further down.

## Protocol decisions

### Extension ↔ host: `bridge-native@1`

Chrome's native messaging framing: a 4-byte **native-endian** length,
then UTF-8 JSON. A message over 1 MiB is refused **from the length
alone**, before any buffer is allocated. `framing.rs` proves this with a
counting reader that shows only the four header bytes are consumed.

| Extension → host | Host → extension |
|---|---|
| `hello` — protocol, extension id, browser-profile instance | `ready` — protocol, identity key, origin, `pause_hidden_after_ms` |
| `attach` — one observation | `attached` — the attachment state |
| `update` — navigation, visibility, re-probe | `request_text` |
| `text` — the answer, with a fresh account probe | `refused` — a named reason |
| `pause` / `detach` — with a cause | `detach` — sharing is over |

### Ownership and identity

- `<data root>/bridge/<identity-key>.lock` is held exclusively (`fs4`) for
  the host's lifetime. A second host reports the first and exits 8. The
  lock file is persistent and removed only by `identity remove`.
- A stale socket is unlinked **only** by a process that already holds that
  lock, so a live endpoint is never removed by a newcomer.
- The host holds the SPEC §10 **shared** identity lock while it runs.
  `identity remove` sends `release` first; the host detaches, tells the
  extension, and exits. If it does not let go within five seconds,
  `identity remove` reports the identity busy and changes nothing.
- The host re-reads `identity.json` every two seconds and stops if the
  identity is gone or was replaced.

### Zones

The content script classifies **before** it reads. The route gives one
classification and the page's frames give another; the stricter wins.

| Zone | What leaves the browser |
|---|---|
| `open` | Route, sanitized URL, title; text only when asked |
| `graded` | The same. Grades come from the API, never scraped |
| `assessment` | Nothing at all |
| `external` | Nothing at all |
| `unknown` | Nothing at all |

For the three opaque zones the bundle carries no route ids, no URL, no
title and no text, and `content_reason` is `zone_opaque`.

### Text release

Metadata is served from the broker's memory: no probe, no page read.
Text is released only on `include_text`, only after a fresh probe the
broker verifies against the identity, and never from an opaque zone.
Hidden inputs, credential-looking field names, and capability parameters
(`verifier`, `Signature`, `X-Amz-*`, `token`, `sig`, `Policy`, `Expires`)
are stripped in the page. The payload is bounded at 64 KiB UTF-8, cut on a
character boundary, with `truncated: true`.

### Reasons and exits

| Reason | Exit |
|---|---|
| `not_attached`, `paused`, `validating`, `account_mismatch`, `bridge_unavailable`, `stale_generation` | 8 |
| `zone_opaque` | 0 — the attachment is healthy; the page simply carries nothing |

## Readings chosen where the report was silent

Each one takes the option that exposes less, and never joins API and
browser data without a verified account. All twelve are also named in
`docs/companion.md`.

1. **`scripting` is declared** — the one deviation from "activeTab and
   nativeMessaging only". Chrome's own documentation states
   `chrome.scripting.executeScript` works under `activeTab` *only if the
   `"scripting"` permission is also declared*. It grants no host access of
   its own; every injection still needs the per-gesture grant. The
   alternative, a declarative `content_scripts` entry, would inject into
   every Canvas page whether or not the person asked.
2. **The probe runs in the content script**, not the service worker, so it
   is same-origin and needs no host permission at all.
3. **An unrecognized frame makes the whole page `unknown`**, and `unknown`
   exposes nothing. A frame nobody recognizes could be an assessment.
4. **`attachments.list` carries no attachment id and no page content.**
   The id is the capability, so a listing must not hand it out. Status
   names state, origin, account id and zone, never the page.
5. **A consumer handle is set by the adapter, never by a model.** A
   `context.here` from `canvas mcp` always carries the calling host's own
   handle, so a stolen id does not serve another consumer.
6. **`context.detach` gives up only the caller's share.** Ending the
   attachment for everyone is `canvas bridge detach`, a human act.
7. **Reading `/context/<handle>` attaches nobody.** Until that consumer
   calls `context.attach` the resource answers `not_attached`, and a
   subscription never opts anybody in.
8. **`context.attach` returns the handle and the state, never the page.**
   Opting in and reading are two decisions.
9. **Browser context is never cacheable**: `ttl_ms` is `0` and the bundle
   carries no freshness row, so nothing downstream can serve a stale
   observation as a fact.
10. **`zone_opaque` is not a refusal** and exits 0. Every other reason
    exits 8.
11. **`api` and `browser` are separate documents.** `api` holds whole §7
    envelopes with their own freshness; a browser extract never updates
    one.
12. **The socket path is length-checked.** A `sockaddr_un` holds about a
    hundred bytes. A data root deep enough to overrun it now produces a
    named local failure that says to set `CANVAS_DATA_ROOT` somewhere
    shorter, instead of an opaque `bind` error. This was a real failure
    hit while writing the integration tests, not a hypothetical.

## Verified in real Chrome: nothing

**No flow was run in a real Chrome on this machine.** Two things stood in
the way and neither was worked around:

- installing the native-messaging manifest writes into the user's own
  Chrome profile directory, and
- the account probe and the API side both need a real Canvas account,
  which this environment does not have.

So the following are **untested and unverified here**: load unpacked, the
toolbar gesture, a same-origin navigation keeping the `activeTab` grant, a
cross-origin navigation revoking it, a Canvas account switch producing
`account_mismatch`, two tabs, and a broker restart driven by Chrome rather
than by a test. Every Chrome-facing rule rests on Chrome's documentation
plus the tests below, not on observation.

**Windows** is compiled and unit-tested — the pipe name and its SDDL
descriptor — but no pipe was created and no registry key was written.

`docs/companion.md` states all of this and gives the person the exact
procedure and expected result for each check.

### What *is* verified, against real processes

| Acceptance row | Where |
|---|---|
| Framing bounds; oversize length rejected before allocation | `canvas-core::bridge::framing` (counting reader) |
| Exact extension id and origin checks, end to end | `tests/bridge.rs` — wrong id, non-extension origin, no origin: each exits 8 and binds no endpoint |
| Failed or redirected identity probe refuses text | `canvas-core::bridge::state` |
| Account mismatch refuses attachment | `canvas-core::bridge::state` |
| Two tabs | `one_identity_holds_one_attachment` |
| Two browser profiles | `a_second_browser_profile_cannot_inherit_the_attachment` |
| Two consumers, only the opted-in one sees it | `tests/bridge.rs` over two real `canvas mcp` processes, plus the broker unit test |
| Stale document and navigation generation rejected | `stale_document_and_navigation_messages_are_rejected` |
| No capture before zone classification; nothing from opaque zones | `extension/test/extract.test.js` (a watched document proves no selector runs) and the broker tests |
| No secret, cookie file, or token in any message | `tests/bridge.rs` asserts over **every byte that crossed either pipe** |
| Byte bounds on character boundaries | `canvas-core::bridge::text` and `extension/test/sanitize.test.js` |
| Unavailable attachment and absent broker exit 8 with the right reason | `tests/bridge.rs`, `commands/here.rs` |
| Broker restart: stale socket cleaned only under ownership, live endpoint never unlinked | `a_restarted_broker_clears_only_a_stale_endpoint` |
| `identity remove` with a live host | `identity_remove_releases_a_live_host` (completes after release) and `a_live_host_that_does_not_answer_reports_busy` |
| `npm test` covers routes, zones, sanitizer, bounds with fixture HTML | `extension/test/` |
| README/clap parity | `tests/readme.rs` |
| `here@1` and `bridge@1` snapshots | `registered_envelopes.snap` |

## Dependencies for Appendix A

**No new crates.** Nothing to add to Appendix A.

The only manifest change is the `net` feature on the existing `tokio`
dependency of `canvas-lms-cli`, which is what the brief directed instead
of adding an IPC crate. Everything else reuses crates already pinned:
`fs4` for the ownership lock, `serde`/`serde_json`, `uuid`, `tempfile`.
No `unsafe` was added anywhere.

The `extension/` package declares **no dependency at all** — not runtime,
not dev. `npm test` runs Node's built-in test runner against a fixture DOM
written for this package.

## Gates

Every gate in the brief, run after the final merge of `main`.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo nextest run --all-features` | **765 tests run: 765 passed**, 0 skipped |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | clean |
| `(cd extension && npm test)` | **33 tests, 33 pass**, 0 fail |
| `cargo xtask bench --runs 3` | every SPEC §13 target met |
| `cargo xtask bench --bridge --runs 3` | warm `here` p50 0.026 ms, p95 **0.026 ms** (target 100) |
| `git status --short` | empty |

### The report that ships

`docs/bench.md` was regenerated a third time with
`cargo xtask bench --mcp --bridge --runs 3`, so the file carries both the
agent surface and the browser companion. Running the two gate commands
verbatim would have left the file without the other lane's `--mcp`
section.

| Metric | Load | p50 ms | p95 ms | Target p95 |
|---|---|---:|---:|---:|
| cached `todo`, first output | idle | 8.9 | 9.8 | 150 |
| cached `todo`, full run | idle | 9.8 | 10.6 | 250 |
| cold start | idle | 12.4 | 12.7 | 400 |
| cached `todo`, first output | download | 9.1 | 9.5 | 150 |
| cached `todo`, full run | download | 9.8 | 10.2 | 250 |
| cold start | download | 12.6 | 12.8 | 400 |
| warm `todo.list` over stdio | mcp | 3.1 | 3.6 | 100 |
| warm metadata `here` over the socket | bridge | 0.030 | 0.030 | 100 |

Catalog: **33 tools, 243,653 bytes, ~60,925 tokens** per `tools/list`.
One `here` answer is **648 bytes** on the wire.

Three things are outside the bridge number by design: the account probe
(it runs in the browser, and only when text is requested), the API side of
`ContextBundle@1` (already measured by the §13 metrics), and process start
(the client connects to the socket directly, the way a resident
`canvas mcp` does).
```
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m7a`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M7-a):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M7-a.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M7-a.md`
