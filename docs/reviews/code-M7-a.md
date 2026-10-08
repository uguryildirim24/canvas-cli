# Code review :  M7-a (companion attachment and broker: `extension/`, native host, `canvas bridge`, `canvas here`), branch `lane/w2`

Reviewer: Claude Opus 5 (high). Base: `2bc0bd0` (merge of `main` into
`lane/w2`). Package brief: `tasks/m7a-companion-broker.md`. Reviewer brief:
`tasks/review-code-m7a.md`. Contract: `docs/agent-ux/REPORT.md` §3.1, §3.3
steps 1-4 and 6, §3.4 (all of it) and §3.5 (zones); `docs/SPEC.md` §8, §10
(identity lock and removal), §14, §15, Appendix A and Appendix D.

## Verdict

**MERGE.** The trust boundary holds where it matters most: no cookie, no PAT,
no capability parameter, no hidden input, and no byte from an `assessment`,
`external` or `unknown` zone can reach the native pipe or the broker socket :
the extension classifies the zone from the frames' attributes before a single
content selector runs, the host re-derives the zone from the sanitized URL and
takes the stricter reading, and text is stored only when a probe arriving with
it reports the identity's own user id. Nine `review(M7-a):` commits fix seven
real defects. Two were security defects: any MCP session could read
`canvas://…/context/<another-consumer>` and receive that consumer's whole
browser bundle :  page title, URL, route ids and verified account :  and a
socket client holding neither the attachment id nor an opt-in could make the
companion re-probe the account and read the page on its behalf. One was a
correctness defect that made the host's "entering an assessment pauses
sharing" rule unreachable from a live companion. All six gates are green at
769 tests, both benches meet every target, and the docs are honest that
nothing in this package was run in a real Chrome. Four items are listed under
"Needs a decision"; none blocks the merge.

## Gate results

Run with `CARGO_TARGET_DIR=<checkout>`
at `ffffb0b` (after the fixes).

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass |
| `cargo nextest run --all-features` | pass :  769 tests run, 769 passed, 0 skipped |
| `cargo deny check` | pass :  advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | pass |
| `(cd extension && npm test)` | pass :  33 tests, 33 pass, 0 fail |
| `cargo xtask bench --runs 3` | pass :  every SPEC §13 target met |
| `cargo xtask bench --bridge --runs 3` | pass :  warm `here` p50 0.062 ms, p95 0.062 ms (target 100) |
| `git status --short` | empty |

The two bench gates were run to scratch files (`--doc`), because either one on
its own rewrites `docs/bench.md` and would drop the other lane's `--mcp`
section. `docs/bench.md` was then regenerated once with
`--mcp --bridge --runs 3`, which keeps all three: warm `here` over the socket
p50 0.030 ms, p95 0.030 ms; warm `todo.list` over stdio p50 2.4 ms, p95
2.6 ms; catalog 33 tools, 243,653 bytes.

Nothing was installed into Rolf's Chrome profile and no command touched
Rolf's identity or credentials. Every test and both benches run against
`tempfile` data roots with `CANVAS_BRIDGE_HOME` pointed at a scratch
directory, and `bridge install` was never run without it.

## Defects found and fixed

| # | Sev | Where | What was wrong | What I changed | Commit |
|---|---|---|---|---|---|
| 1 | high | `crates/canvas-cli/src/mcp/resources.rs:172`, `crates/canvas-cli/src/mcp/server.rs:419` | `resources::read` passed the consumer handle from the resource URI straight to `here::handle`. Any MCP session could read `canvas://<key>/<gen>/context/<other-consumer>` and get that consumer's whole browser bundle: origin, sanitized URL, page title, course and assignment ids, document id, navigation generation, and the verified account. REPORT §3.2 makes the context resource answer `not_attached` until attachment is explicit :  for the reader, not for somebody else. | The session's own handle now reaches `resources::read` the same way it reaches `catalog::dispatch`, and a `context/<handle>` naming another consumer answers exactly what an unattached consumer reads. `only_the_consumer_that_attached_reads_the_bundle` now has beta read alpha's resource; it fails on the old code with alpha's page in hand. | `b87cb55`, shape corrected in `7989501` |
| 2 | medium | `crates/canvas-cli/src/bridge/host.rs:477` | `here` asked the extension to re-probe the account and extract the page **first** and consulted `Broker::context` :  the only place the attachment id and the consumer opt-in were checked :  afterwards. A socket client holding neither could make the companion read the page and issue the Canvas probe on its behalf, and received `account_mismatch` before any entitlement was established. It never got the text; causing the read at all is what REPORT §3.3 step 4 puts behind the opt-in. | The capability check is now `Broker::may_read`, called by `context` as before and by the host before `request_text`. It is a capability check only, so the `paused`/`validating` state refusals are unchanged. New unit test `an_unentitled_caller_is_refused_before_anything_is_read`. | `d1835fd` |
| 3 | medium | `crates/canvas-core/src/bridge/state.rs:600` (`zone_of`) | `zone_of` read a missing URL as `Zone::Unknown`. But a missing URL is exactly what an opaque document sends :  `content.js` nulls `url`, `title` and the route whenever the zone is opaque :  so every real assessment arrived as `assessment` and was stored as `unknown`. Both are opaque, so no content escaped; what was lost is the distinction `update` turns on, making the `zone == Assessment` pause branch unreachable from a live companion and reporting an assessment as merely unrecognized in `bridge status` and `canvas here`. The existing zone tests passed because their fixture sends a URL for opaque pages, which the companion never does. | With no URL to classify, an already-opaque classification stands and anything else still reads `unknown`, so the extension still cannot talk the host into a more permissive zone. New test `an_assessment_that_carries_no_url_is_still_an_assessment`, which fails on the old logic. | `c75e8d9` |
| 4 | medium | `crates/canvas-cli/tests/bridge.rs:397` | `nothing_on_the_wire_carries_a_secret` :  the brief's "assert on the wire log" acceptance row :  checked that `PAGE_SECRET`, `verifier` and `X-Amz-Signature` never appear on the pipes, but the fixture observation carries no query string and no extract, so none of them could have appeared however the host behaved. The assertion was passing on an empty room, and `docs/companion.md` claimed it as evidence. | The companion now reports a file-download URL carrying `verifier=<planted secret>` and `X-Amz-Signature=deadbeef` beside an ordinary `wrap=1`. The test asserts the host's own half of the wire echoes neither, `canvas here --json` carries neither, and `wrap=1` survives :  so the check cannot be satisfied by dropping the URL. The host's messages are now logged separately from the extension's, because only the host's half is this package's to promise. | `33a7e11`, renamed for clippy in `cd4b78a` |
| 5 | low | `extension/src/content.js:61,110` | `observe` and `extract` classified the zone from `location.pathname`, awaited the account probe, then read `location.href`, the title and the page text. A same-document navigation during the probe :  Canvas routes several of its own pages that way :  would pair one document's zone with another document's address and content. | Both capture the address alongside the frames, classify from that, and check it is still the address after the probe; a page that moved is answered as `unknown`, which reads nothing. The host's re-derivation from the URL it receives is unchanged. Not observed in a browser; no flow in this package has been. | `ed27754` |
| 6 | low | `crates/canvas-cli/src/bridge/client.rs:20` | `client::TIMEOUT` and `host::TEXT_TIMEOUT` were both five seconds, so a page taking about that long to re-probe and extract would have the client give up first and `canvas here --text` would report `bridge_unavailable` :  "no canvas bridge host is running" :  for a live broker about to answer `validating`. | The client waits fifteen seconds, and a test pins the ordering so the two constants cannot drift back together. | `f9a0d30` |
| 7 | low | `crates/canvas-cli/src/config.rs:438` | `config set bridge.pause_hidden_after` validated by parsing the value and comparing the result to the default, so every duration that happens to be ten minutes was rejected as malformed :  `600s` was refused :  while `10m` passed only as a named special case. The key had no test at all. | Parsing and defaulting are two functions now: `parse_pause_hidden_after` answers whether the value is a duration, `pause_hidden_after_ms` falls back for a config file this build cannot read. New end-to-end test: the default and a configured value each reach the companion in `ready`, and a count with no unit, a unit with no count, zero, and an unsupported unit are each a usage error. | `4b292a5` |

`a80e0c6` records the five doc lines these fixes changed in
`docs/companion.md`; `ffffb0b` re-records `docs/bench.md` on the reviewed
code.

## What I attacked, and what held

- **Secrets on the wire.** The extension strips capability parameters
  (`verifier`, `Signature`, `X-Amz-*`, `token`, `sig`, `Policy`, `Expires`,
  case-insensitively and `x-amz-` as a prefix), URL credentials and the
  fragment; the host applies `sanitize_url` again to every URL it stores,
  because it must not depend on the extension being the one it installed.
  Hidden inputs, `aria-hidden`, inline `display:none`, `password` inputs and
  any element whose `name`, `id` or `autocomplete` looks like a credential are
  skipped, and the content allowlist means a Canvas page this release does not
  know contributes nothing rather than everything. The PAT never enters the
  extension at all: there is no fetch proxy and no cookie import, and the one
  Canvas request is a fixed same-origin `GET /api/v1/users/self` with
  `redirect: "error"` whose body is reduced to `{ user_id, observed_at }`
  inside the content script.
- **Classify before you read.** `extractText` returns on the opaque check
  before it touches a selector, and `extension/test/extract.test.js` proves it
  with a watched document that counts `querySelectorAll` calls: zero.
- **Text release.** Metadata is served from the broker's memory and triggers
  no probe. Text is stored only when `Broker::text` verifies the accompanying
  probe against `identity.user_id`, the document id and navigation generation
  both match what the broker currently holds, and neither the message's zone
  nor the stored zone is opaque. A missing account is `account_mismatch`, so a
  failed or redirected probe releases nothing.
- **Callers.** A wrong extension id, a non-extension caller origin and no
  origin at all each exit 8 and bind no endpoint :  checked before a byte is
  read from the pipe, and again in `Broker::hello`. The 1 MiB native-message
  bound is enforced from the four header bytes, and a counting reader proves
  only those four were consumed. The 8 KiB request line is bounded as it
  arrives, in `read_bounded_line`, not after assembly.
- **Ownership.** `<data root>/bridge/<key>.lock` is `fs4`-exclusive for the
  host's lifetime; a second host reports the first and exits 8 without
  unlinking the live endpoint; the stale socket is unlinked only through a
  method that requires an `Ownership`, which requires the lock; the lock file
  itself is removed only by `identity remove`, and the root identity lock
  under `locks/` is never touched. `identity remove` sends `release`, waits
  five seconds, and reports the identity busy without changing anything if the
  host keeps holding.
- **Bounds.** The 64 KiB ceiling is applied in the extension and again in the
  host, on character boundaries, across the selection and the excerpt
  together; both sides are tested with four-byte characters so no multiple of
  the budget lands on a boundary by accident.
- **Install.** `bridge install` writes exactly one file :  the native-messaging
  host manifest, mode `0600`, written to a temp path and renamed :  into the
  browser's per-user `NativeMessagingHosts` directory, with the exact
  extension id in `allowed_origins` and an absolute, canonicalized binary
  path. No browser profile, preferences file or cookie store is opened, and
  the three browsers do not share a directory.
- **Permissions.** The shipped manifest asks for `activeTab`,
  `nativeMessaging` and `scripting`, with no `host_permissions`, no
  `content_scripts`, no `cookies`, `webRequest`, `tabs`, `storage` or
  `externally_connectable`. `tests/companion.rs` asserts all of it.
- **Honesty.** `docs/companion.md` §"What was run, and what was not" states
  plainly that the companion was never loaded into a browser here and names
  each untested flow. I found no claim in the docs, the README or `SKILL.md`
  that exceeds what the tests prove :  the one that did (defect 4) is now
  backed by a test that could fail.

## Needs a decision

1. **`scripting` is a fourth permission the design did not name.** REPORT §3.3
   and the brief say `activeTab` and `nativeMessaging` only. Chrome requires
   `"scripting"` to be declared for `chrome.scripting.executeScript` even
   under `activeTab`, so the alternative is a declarative `content_scripts`
   entry, which needs `host_permissions` and injects into every Canvas page
   whether or not the person asked. The worker took the narrower option and
   recorded the deviation in `docs/companion.md` and in
   `tests/companion.rs`. I agree with the reading and did not change it, but
   it is a departure from a written contract and belongs on the record.
2. **`docs/SPEC.md` Appendix A does not list the tokio features this uses.**
   Line 759 reads `tokio (rt, macros, fs, time, sync)`; the workspace already
   uses `io-util` and `signal` on `main`, and this package adds `net`. No new
   crate is added and `cargo deny` is clean, so nothing is unvetted :  but the
   appendix is now three features out of date. The brief forbids editing
   `docs/SPEC.md`, so I left it.
3. **A socket client may name any consumer handle.** `Op::Attach` and
   `Op::Here` take the consumer as a field, so a process that can open the
   socket can opt in under any name and read what that name may read. REPORT
   §3.2 says so directly :  "Consumer handles express routing within Rolf's OS
   trust domain, not isolation from another unrestricted process" :  and the
   socket is `0600` inside a `0700` directory, which is the boundary. Defect 1
   was the case where this leaked *across* the MCP adapter, where the handle
   is not the caller's to choose; the socket itself is by design. Worth
   restating in the SPEC when the companion is written up, so the boundary is
   not read as an oversight.
4. **`update` accepts a different document id at the same navigation
   generation.** `Broker::update` rejects a generation lower than the current
   one, but a message naming an older document with an equal generation is
   treated as a new document and accepted. The extension increments the
   generation on every committed navigation, so a genuinely late message
   always carries a lower one and is refused; the acceptance is unreachable
   from the shipped companion. Making it an explicit refusal would mean
   deciding whether an equal-generation, different-document message is stale
   or a legitimate same-document replacement, which the report does not say. I
   left it rather than guess.
