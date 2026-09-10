# M7-b — Presence, panel approvals, follow: the side panel, `context.note`, `context.follow`, `open --follow` (Claude Opus, lane w2)

Post-v1 package from the agent-UX design. Read `docs/agent-ux/REPORT.md`
§3.2 (the presence and navigation rows, the `context.follow` stale
generation exit row, the annotations paragraph on navigation), §3.3
(steps 5 and 7), §3.4 (the side panel is an extension-owned surface, not a
Canvas DOM overlay), §3.5 (the approval handle; the `panel` channel; "the
host's human-response path or private panel/TTY event provides approval";
echoing a handle is not proof), §3.6 (events as the panel's status feed),
§4 (the M7-b row and its acceptance column), and the sources S12, S13,
S15 it cites (read the Chrome sidePanel API and the pinned discussion
controller for the read-state side effect of navigation). Then
`docs/SPEC.md` §5 (`open`), §6, §7, §12.2 (journal states the panel must
show as they are), §14, §15, Appendix D. Precedence: REPORT §3.3–§3.5
define the panel and the approval channel; SPEC §12.2 defines what a
journal state means and is unchanged. Existing code: M7-a (`extension/`,
`canvas bridge host`, the broker protocol `bridge-ipc@1`, attachments,
`canvas here`), `canvas-core::plan` (M6-a), the MCP catalog and the
elicitation round trip (M6-b), `canvas-core::events` and `watch` (M6-c),
`docs/companion.md`. Read their public APIs first. Not in this package:
any Canvas write, any new read dataset, remote transports.

## Deliverables
1. **Side panel** (`extension/`, `sidePanel` permission added; opened by
   the same gesture as the attachment): shows the attachment state and
   consumer, the current page's API facts (through `here`, metadata only,
   no text), freshness, the pending journals and the latest receipts for
   the current course and assignment, and the notes feed. It has no model
   or chat backend and never talks to Canvas itself; everything comes from
   the native host over the M7-a protocol. Journal and receipt states are
   displayed with the exact §12.2 names; `matched` says "attribution
   unproven", `outcome_unknown` says so, nothing is ever shown as done that
   the journal does not say is done.
2. **`context.note(attachment_id, generation, text, source_refs)`** (MCP;
   CLI `canvas note --attachment ID --text …`): a bounded (≤ 8 KiB) inert
   note rendered as text with a sanitized subset of Markdown (no HTML, no
   links that are not `https://<the granted origin>/…` or `canvas://`
   source refs, no scripts, no images); a stale generation is refused
   (exit 8, `reason: stale_generation`); notes are held by the broker for
   the attachment's lifetime and erased on detach; a note can never
   approve, decline, or cancel a plan, and a test proves that a note whose
   text or refs look like an approval payload changes nothing.
3. **`context.follow(attachment_id, generation, target)`** (MCP; CLI
   `canvas open <target> --follow [--attachment ID]`): resolves the target
   through the existing `open` resolver (never fetches; cross-origin exit
   6), then asks the extension to navigate the attached tab within the
   granted origin; a stale generation is refused (exit 8); the result
   separates **dispatch acknowledgement** (the extension accepted the
   navigation; target p95 < 300 ms, measured) from **load outcome**
   (`loaded`, `failed`, `unknown`) reported as a later state; the
   annotation is not read-only and the human output says navigation can
   have Canvas page side effects (a discussion page marks itself read;
   S13), distinct from the API-preview guarantee of the reads.
4. **Panel approvals** (`approval.channel = "panel"`): when a plan awaits
   approval (a prepared plan whose handle was issued to a consumer), the
   panel shows the exact frozen plan (target, payload digests, files with
   sizes and hashes, text preview, baseline) and offers approve, decline,
   cancel. The decision travels only over the native-messaging path from
   the extension to the host and carries the same handle the panel was
   shown; the host validates handle, digest, identity generation, and
   consumer, then calls `plan::approve(channel "panel")`, `decline`, or
   `cancel`; a page script, a note, a content-script message, or a socket
   client can never forge it (the socket protocol has no approval message;
   a test proves each forgery path changes nothing). Approval events are
   private: the panel shows them to the person only, and the events log
   records them as `plan.approved|declined|cancelled` with no payload
   beyond ids.
5. **Status feed**: the panel subscribes to the events log through the
   host (M6-c cursor rules; `resync_required` shows as "refresh") and
   updates journals and receipts as they change; the extension never
   polls Canvas.
6. **Docs and measurements**: extend `docs/companion.md` with the panel,
   notes, follow, and the approval flow, and — as before — exactly what
   you ran in real Chrome on this machine (note display, malicious markup
   inert, follow acknowledgement and load, stale follow refused, panel
   approve/decline/cancel, forged approval rejected) and what stayed
   untested; extend `cargo xtask bench --bridge` with the follow
   acknowledgement p50/p95 and append the numbers to `docs/bench.md`.
   Registry: `note@1`, `follow@1`; MCP tools `context.note` and
   `context.follow` in the catalog allowlist test and the skill.
7. **Tests** (REPORT §4 M7-b acceptance, each explicit): a note from an
   existing agent displays without any model backend; malicious markup is
   inert (script, HTML, javascript: URLs, off-origin links, oversized
   text); notes and navigation are generation-bound; stale follow exits 8;
   exact plan approve, decline, cancel through the panel path; no
   page-forged approval (every forgery path); observed and unknown receipt
   states shown as distinct; follow acknowledgement separated from load;
   the API-preview versus browser read-state side effect is distinguished
   in the output and in the docs; `npm test` covers the panel's renderer
   and sanitizer with fixture notes; README/clap parity; catalog and skill
   parity; `note@1` and `follow@1` snapshots.

## Rules
- You own `extension/**` (panel files added this round), the host-side
  note, follow, and approval handling under `crates/canvas-cli/src/bridge/**`
  and `crates/canvas-core/src/bridge/**`, `commands/{note,open}.rs`
  changes, the registry entries `note@1` and `follow@1`, the two MCP
  tools, the `bench --bridge` extension, and `docs/companion.md`. Shared
  files (command enum, registry, README, catalog, skill, events kinds):
  add your entries, keep every other lane's, never rename or reorder. No
  migration is expected; the three approval event kinds are additions to
  the M6-c kind list, not a schema change.
- Work on branch `lane/w2` in this worktree with
  `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/w2`. Commit
  as you go with conventional messages. Before reporting, `git merge main`
  (resolve, rerun gates). Do not push. Do not merge into `main`.
- Do not touch `docs/` (except `docs/companion.md` and the `docs/bench.md`
  append) or `tasks/`. Where the report is silent, choose the reading
  under which nothing on a web page can approve anything and the panel
  never shows more certainty than the journal holds; name each choice in
  `docs/companion.md` and in your final message.

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
Finish with `git status --short` and reply with the marker `DONE M7-b` on
its own line.
