# SPEC v0.9 consolidation, pass 2 — companion (§24), writes (§25), final verification (Claude Opus, lane w3)

A documentation package, the second and last pass of the v0.9
consolidation. Pass 1 (`docs/reviews/spec-v0.9-pass1.md`, merged) wrote
§20–§23 and left placeholders for §24 and §25. M7-a, M7-b, and M8-b are
now on `main`. Read `docs/SPEC.md` (all of it, v0.9 draft),
`docs/SPEC-CHANGES-v0.9.md`, `docs/agent-ux/REPORT.md` §3.3–§3.5,
`docs/companion.md`, `docs/writes-v2.md`, `docs/agent-hosts.md`,
`docs/bench.md`, the reviews `docs/reviews/code-M7-a.md`, `code-M7-b.md`,
`code-M8-b.md`, the briefs `tasks/m7a-companion-broker.md`,
`m7b-panel-presence-follow.md`, `m8b-discussion-inbox-writes.md`, and then
the code for every statement (the `extension/` manifest and scripts, the
bridge host and broker, the plan kinds, the operation journal, the MCP
catalog, the skill). Same rules as pass 1: the code on `main` and its
tests are the truth for "as built"; the REPORT is the truth for intent;
write what is built and add a §19 item for every difference; never resolve
a §19 item; make no decision of your own.

## Deliverables
1. **§24 Browser companion** (from M7-a and M7-b as built): permissions
   and what the extension may not do, the gesture and attachment
   lifecycle, zones, the account probe, text release rules and bounds,
   the native host, the broker ownership lock and endpoint, the
   `bridge-ipc@1` protocol, consumer routing and its trust boundary (§19
   item 31), `canvas bridge install|host|status|detach`, `canvas here`,
   `canvas note`, `canvas open --follow`, the side panel, panel approvals
   (`approval.channel = "panel"`), the status feed, `here@1`, `bridge@1`,
   `note@1`, `follow@1`, the `context.*` MCP tools and the `/context`
   resource, and, plainly, what has and has not been run in a real
   Chrome (from `docs/companion.md`).
2. **§25 Discussion and inbox writes** (from M8-b as built): plan kinds,
   the operation journal and its states, admission and owner locks,
   recovery, `reconcile`, attribution (`accepted|observed|unproven|none`),
   the refusal list, attachments, receipts extension, `operation@1`,
   `operation_reconcile@1`, the MCP tools, the skill workflow and the
   course-policy boundary, and the "accepted is not delivered" rule.
3. Extend the tables again: §5 (every new command with its class), §9
   (bridge paths, `bridge.*` config keys, journal lock names), §10
   (mutation epochs for the new datasets, pending hook coverage), §14
   (new `reason` values), migrations (`0004_operations`), Appendix A
   (re-verify every row against `Cargo.toml`/`Cargo.lock`, including
   whether tokio `net` is now a production feature), Appendix B, Appendix
   D. Fold `docs/companion.md` and `docs/writes-v2.md` in and reduce each
   to a pointer, as pass 1 did with `docs/reads-v2.md`.
4. Reconcile §19 with the code now on `main`: an item that the code has
   since fixed (for example item 36 if `canvas schema --list` now names
   real commands) gets a one-line "resolved by <commit>" note appended,
   not deleted and not reworded; add items only for new REPORT-versus-code
   differences.
5. Update `docs/SPEC-CHANGES-v0.9.md` and drop "(draft)" from the header
   only if every placeholder is gone and every section describes built
   code; otherwise keep "(draft)" and say why in the changes file.

## Rules
- You may edit only `docs/SPEC.md`, `docs/SPEC-CHANGES-v0.9.md`, and the
  two pointer reductions. No code, no tests, no other docs, no `tasks/`.
- Every normative sentence must be true of the code on `main`; when
  unsure, read the test or leave the sentence out. Never invent; never
  resolve a §19 item.
- Work on branch `lane/w3` in this worktree with
  `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/w3`. Commit
  as you go with `docs(spec):` messages. Before reporting, `git merge
  main`. Do not push. Do not merge into `main`.

## Gates
```
cargo fmt --all --check
cargo nextest run --all-features   # unchanged count proves no code was touched
```
Finish with `git status --short` and reply with the marker `DONE SPEC-v0.9-pass2`
on its own line.
