# SPEC v0.9 consolidation, pass 1 — write the post-v1 contracts into `docs/SPEC.md` as built (Claude Opus, lane w3)

A documentation package. `docs/SPEC.md` v0.8 is the v1 contract; the
post-v1 packages were built from `docs/agent-ux/REPORT.md` and from
per-package briefs and contract documents. The owner must be able to read
one document in the morning. Your job is to fold what is **on `main`
now** into the SPEC, with no new decision of your own. Read
`docs/SPEC.md` (all of it), `docs/agent-ux/REPORT.md` §3–§4,
`docs/reads-v2.md`, `docs/agent-hosts.md`, `docs/bench.md`, the reviews
`docs/reviews/code-M6-a.md`, `code-M6-b.md`, `code-M6-c.md`,
`code-M6-c2.md`, `code-M8-a.md`, `code-M8-a2.md`, `code-M8-a3.md`, the
briefs `tasks/m6a-plans-approval.md`, `m6b-agent-adapters.md`,
`m6c-coordinator-watch.md`, `m8a-richer-reads.md`, and then the code
itself for every statement you write (the registry, the command enum,
`canvas-core::plan`, `coord`, `events`, the MCP catalog, the skill).
Precedence: the code on `main` and its tests are the truth for "as
built"; the REPORT is the truth for intent; where they differ, write what
is built and add a §19 item that names the difference. Not in this pass:
M7-a/M7-b (companion) and M8-b (writes), which are still in flight; leave
a one-line placeholder for each where its section will go.

## Deliverables
1. Bump the header to **v0.9 (draft)** and add an Appendix C paragraph
   "What the post-v1 rounds changed", listing the packages and their
   review verdict files.
2. New sections, written in the SPEC's own style (short normative
   sentences, tables for contracts, the `@n` schema convention):
   - **§20 Operation plans and approval** (from M6-a as built): the
     `plans` and `approval_handles` tables, plan states, the 15-minute
     admission expiry, the approval record, the handle binding, the
     execute rules, the human `submit` flow on top, `plan@1`, exit
     mappings, and the `replayed` field (§19 item 17).
   - **§21 Agent surface** (from M6-b, M8-a2 as built): `canvas schema`
     (per-form pages, §19 items 20 and 33), `canvas mcp` (protocol
     versions, the exact tool catalog with annotations, results,
     `ttlMs`, resources by identity and generation, the approval round
     trip, the catalog size number and §19 item 19), the skill and its
     workflows, the host matrix pointer to `docs/agent-hosts.md`.
   - **§22 Coordinator, events, watch, notify** (from M6-c, M6-c2, M8-a3
     as built): permits, the shared governor row and its §11 rules,
     refresh single-flight, foreground interest and priority (§19 items
     23, 25, 29), the events tables, kinds, baseline and replay rules,
     `event@1`, `watch@1`, `canvas watch --jsonl`, `canvas notify`
     (stdout only; §19 on desktop backends), MCP subscriptions.
   - **§23 Richer reads** (from M8-a as built): pages, page, syllabus,
     discussions, discussion, inbox, inbox show, inbox unread-count, the
     rubric extension, coverage and truncation rules, failure rules,
     `docs/reads-v2.md` folded in and then reduced to a pointer.
3. Extend the existing tables rather than duplicating them: §5 command
   table (every new command with its class), §9 paths and config keys
   (`ttl_pages`, `ttl_discussions`, `ttl_inbox`, `bridge.*` placeholders
   only), §10 datasets (the new rows with scope, fetch, TTL, complete
   rule), §14 (no new codes; add the new `reason` values), Appendix A
   (already updated; verify), Appendix B (every new endpoint and query
   string), Appendix D (every new `@1` payload, every additive field
   marked as such), the migration list (`0002_plans`, `0002_reads` cache,
   `0003_events`).
4. §19: keep every existing item verbatim; add items only for a
   difference between the REPORT and the code that no item covers yet.
5. A `docs/SPEC-CHANGES-v0.9.md` list of what you added or changed, one
   line per section, so the reviewer and the owner can check it fast.

## Rules
- You may edit only `docs/SPEC.md`, `docs/SPEC-CHANGES-v0.9.md`, and
  reduce `docs/reads-v2.md` to a pointer after folding it in. No code, no
  tests, no other docs, no `tasks/`.
- Every normative sentence you add must be true of the code on `main`;
  when unsure, read the test that proves it or leave the sentence out.
  Never invent a rule; never resolve a §19 item.
- Work on branch `lane/w3` in this worktree with
  `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/w3`. Commit
  as you go with `docs(spec):` messages. Before reporting, `git merge
  main`. Do not push. Do not merge into `main`.

## Gates
```
cargo fmt --all --check
cargo nextest run --all-features   # unchanged; proves you touched no code
```
Finish with `git status --short` and reply with the marker `DONE SPEC-v0.9-pass1`
on its own line.
