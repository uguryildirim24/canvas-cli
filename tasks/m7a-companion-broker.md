# M7-a — Companion attachment and broker: `extension/`, native host, `canvas bridge`, `canvas here` (Claude Opus, lane w2)

Post-v1 package from the agent-UX design. Read `docs/agent-ux/REPORT.md`
§3.1 (two kinds of context, `ContextBundle@1`, the fixed `users/self`
probe as the sole browser-session exception), §3.2 (the location/routing
rows, the CLI companion row, the `here@1` and `bridge@1` registry rows, the
exit mapping rows for `not_attached|paused|validating` and
`bridge_unavailable`, the `/context/<consumer-handle>` resource), §3.3
(steps 1–4 and 6; steps 5 and 7 are M7-b), §3.4 (**all of it**), §3.5
(zones; the approval boundary), §3.6 (identity leases), §4 (the M7-a row
and its acceptance column), §5 (Lasell origin, host reachability), and the
sources S1, S12, S14, S15 it cites (read Chrome's content-script isolated
worlds, `activeTab`, native messaging, and sidePanel pages; pin the exact
manifest keys you rely on). Then `docs/SPEC.md` §5 (`open`), §8 (canonical
origin, identity key, selection matrix), §9, §10 (identity lock, identity
removal), §14, §15, Appendix D. Precedence: REPORT §3.3–§3.4 define the
companion; SPEC §8 and §10 define identity and locking and are unchanged.
Existing code: `canvas-core::identity` (`Paths`, `IdentityKey`,
`IdentityDocument`, the shared identity lock), `canvas-core::coord`
(M6-c), the command handlers and the MCP catalog (M6-b), `open`,
`crates/canvas-cli/src/output` (registry), `dist-workspace.toml` (M5-b).
Read their public APIs first. Not in this package: the side panel,
`context.note`, `context.follow`, `open --follow` (M7-b); any write.

## Deliverables
1. **`extension/`** (Manifest V3, plain TypeScript compiled with no bundler
   magic, or plain JS; `npm test` runs Node's built-in test runner; no
   runtime dependency): permissions `activeTab` and `nativeMessaging` only,
   no `host_permissions`, no `cookies`, no `webRequest`. A toolbar action
   (and a command shortcut) is the gesture; the service worker connects to
   the native host `com.canvas_cli.bridge` and forwards: browser-reported
   origin, tab id, document and frame ids, a **navigation generation** it
   increments on every committed navigation, route classification
   (`course_id`, `assignment_id`, `topic_id`, `quiz_id`, `page_url`, kind),
   and the result of a fixed `GET <origin>/api/v1/users/self` performed in
   extension code with `redirect: "error"`, `credentials: "same-origin"`,
   `Accept: application/json`; the body is reduced to `{ id }` before it
   leaves the extension. Cookies never leave Chrome; there is no fetch
   proxy. The isolated-world content script classifies the **zone**
   (`open|graded|assessment|external|unknown`, from the route and the
   presence of quiz, assessment, or unknown-tool frames) **before** any
   text is read; `assessment`, `external`, and `unknown` expose nothing.
   Text extraction runs only on request (`include_text`): allowlisted
   text fields, selection, and the visible editor excerpt; hidden inputs,
   credential-looking fields, and capability query parameters
   (`verifier`, `Signature`, `X-Amz-*`, `token`, `sig`, `Policy`,
   `Expires`) are stripped; total payload ≤ 64 KiB UTF-8 on character
   boundaries with `truncated: true`. Cross-origin navigation ends the
   grant (a new gesture is required); a new document enters `validating`
   and old text is erased; hidden-tab pause after `bridge.pause_hidden_after`
   (config, default 10 min); tab close, host loss, or entering an
   assessment pauses/ends sharing. No MAIN-world script in this package.
2. **`canvas bridge host`**: Chrome native messaging (4-byte native-endian
   length prefix, JSON, ≤ 1 MiB per message enforced **before**
   allocation) on stdin/stdout; it is the **broker owner** for one identity
   (the identity of the profile selected by §8; the extension's probed
   `id` must equal the identity's user id, else the attachment is refused
   with `reason: account_mismatch` and no text is ever accepted). Ownership:
   `<data root>/bridge/<identity-key>.lock` (`fs4`, never deleted) held
   for the host's lifetime; a second host for the same identity reports
   the existing owner and exits, never replaces it. Endpoint: Unix socket
   `<data root>/bridge/<identity-key>.sock` (dir `0700`, socket `0600`; a
   stale socket file is unlinked only by a host that holds the ownership
   lock; a live one never). Windows: named pipe
   `\\.\pipe\canvas-cli-<identity-key>` with a user-restricted ACL,
   compiled and unit-tested for the path and ACL builder; say in the docs
   what was not run. The host holds the shared identity lock (§10) for its
   lifetime and answers `identity remove`'s cooperative release request by
   detaching and exiting; the lease and the endpoint are removed with the
   identity, never the root identity lock.
3. **Broker protocol** over the socket (newline-delimited JSON, versioned
   `bridge-ipc@1`): `attachments.list`, `attach { consumer }`, `here {
   attachment_id, include_text }`, `detach { attachment_id }`. One active
   attachment per identity, bound to browser-profile instance, tab, origin,
   account, identity generation, and navigation generation; `attachment_id`
   is an opaque 128-bit value. Consumers opt in with it; the sole
   attachment may be selected by the CLI when only one exists; a message
   carrying an obsolete document or navigation generation is rejected;
   metadata-only reads are served from the broker's last validated state;
   text is released only after a fresh account probe (the extension
   re-probes on request) and never recycled from an earlier page. The
   broker buffers are erased on pause and detach.
4. **`canvas bridge install [--extension-id ID] [--browser chrome|chromium|edge]`,
   `status`, `detach`** (class B; `bridge@1`): `install` writes the native
   host manifest (exact extension id in `allowed_origins`, absolute path to
   the `canvas` binary, `type: stdio`) into the browser's
   NativeMessagingHosts directory for the current user, prints the
   unpacked-extension load steps, and never touches browser profiles or
   cookie files; `status` reports manifest presence, host owner (live or
   absent), attachment state, and the endpoint; `detach` asks the live
   owner to drop the attachment. All three are ordinary class-B commands
   with `--json` per §7.
5. **`canvas here [--attachment ID] [--text] --json`** (class C; `here@1`
   = `ContextBundle@1`): `attachment`, `state`, `consumer`, identity key
   and generation; `api`: typed §7 envelopes for what the route resolved
   (`course@1`, `assignment@1`, `announcement@1`, or M8-a reads when on
   `main`) through the shared handlers with their own freshness; `browser`:
   origin, verified account `{ user_id, observed_at }`, page kind and ids,
   sanitized URL and title, frame and navigation generation, zone,
   observed_at, ttl, and optional `selection`/`text` with lengths and
   `truncated`. Unavailable content carries an explicit `reason`
   (`not_attached`, `paused`, `validating`, `zone_opaque`,
   `account_mismatch`, `bridge_unavailable`) and, for the refusal cases in
   REPORT §3.2, exit 8. A browser extract never updates an API field.
6. **MCP**: `context.attach`, `context.here`, `context.detach` in the M6-b
   catalog with the annotations REPORT §3.2 gives; the resource
   `canvas://<identity-key>/<generation>/context/<consumer-handle>`
   returns the bundle only after that consumer attached; subscriptions
   never attach. Add the three tools to the catalog allowlist test and the
   skill's command list.
7. **Packaging and docs**: the `extension/` build output ships in the
   `cargo dist` archives (extend the M5-b config; `cargo xtask dist-assets`
   if that is where assets are staged); `docs/companion.md` (the one docs
   file you may write besides a `docs/bench.md` append): install steps for
   Chrome with an unpacked extension, the manifest template, the protocol,
   the zones, and — like `docs/agent-hosts.md` — exactly what you ran in
   real Chrome on this machine (load unpacked, gesture, same-origin
   navigation keeps the grant, cross-origin revokes it, account switch,
   two tabs, two consumers, broker restart) and what stayed untested.
   Never claim an untested flow works. Add `cargo xtask bench --bridge`
   (warm metadata `here` p50/p95 over the socket, target p95 < 100 ms
   excluding the probe) and append the numbers to `docs/bench.md`.
8. **Tests** (REPORT §4 M7-a acceptance, each explicit): framing bounds
   (oversize length rejected before allocation); exact extension id and
   origin checks (a wrong id or origin is refused); failed or redirected
   identity probe refuses text; account mismatch refuses attachment; two
   tabs, two profiles, two consumers (only the opted-in consumer sees the
   bundle); stale document and navigation generation messages rejected;
   no capture before zone classification and nothing from opaque zones;
   no secret, cookie file, or token in any message (assert on the wire
   log); byte bounds on character boundaries; unavailable attachment and
   absent broker exit 8 with the right `reason`; broker restart: stale
   socket cleaned only under ownership, live endpoint never unlinked;
   `identity remove` with a live host reports busy or completes after the
   cooperative release; `npm test` covers the route classifier, zone
   classifier, sanitizer, and bounds with fixture HTML; README/clap parity;
   `here@1` and `bridge@1` snapshots.

## Rules
- You own `extension/**`, `crates/canvas-cli/src/bridge/**`,
  `crates/canvas-core/src/bridge/**`, `commands/{bridge,here}.rs`, the
  registry entries `here@1`, `bridge@1`, the three MCP tools, the config
  keys `bridge.pause_hidden_after` and `bridge.extension_id`,
  `docs/companion.md`, and the `bench --bridge` extension. Shared files
  (command enum, registry, README, MCP catalog, skill, `config set`
  allowlist, dist config): add your entries, keep every other lane's,
  never rename or reorder. No migration is expected. New crate
  dependencies must be verified on crates.io, pass `cargo deny`, add no
  `unsafe`, and be listed in your final message for Appendix A; prefer
  tokio's own `net` feature over a new IPC crate.
- Work on branch `lane/w2` in this worktree with
  `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/w2`. Commit
  as you go with conventional messages. Before reporting, `git merge main`
  (resolve, rerun gates). Do not push. Do not merge into `main`.
- Do not touch `docs/` (except `docs/companion.md` and the `docs/bench.md`
  append) or `tasks/`. Where the report is silent, choose the reading that
  exposes less browser content and never joins API and browser data
  without a verified account; name each choice in `docs/companion.md` and
  in your final message.

## Gates
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
(cd extension && npm test)
cargo xtask bench --runs 3
cargo xtask bench --bridge --runs 3
```
Finish with `git status --short` and reply with the marker `DONE M7-a` on
its own line.
