# Code review + fix — M6-c on branch lane/w3 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M6-c
on branch `lane/w3` (worktree `/Users/rolfie/projects/canvas-cli/.worktrees/w3`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /Users/rolfie/projects/canvas-cli/.worktrees/w3`. Read the package brief `tasks/m6c-coordinator-watch.md` and the SPEC sections it
   cites. First run `git merge main` (expect nothing to do; resolve if needed and commit). Then read `git log main..HEAD --stat` and the full diff. Scope: the package is delivered without its item 7 (the MCP `subscriptions/listen` hookup), which waits for M6-b on `main` and will be a separate follow-on; do not count item 7 as missing. Its contract is `docs/agent-ux/REPORT.md` §3.6 (all of it) and §3.4 (identity leases), with `docs/SPEC.md` §10 and §11 unchanged for what a dataset, a TTL, and a governor sample mean. Attack in particular: can two processes ever exceed `api_concurrency` in flight (count on the wiremock side, not in process)? Does the shared governor apply the §11 rules to the shared values (lower sample always, higher only above the watermark, cost pre-charged, refill never above 10/s, cooldown shared) and never invent a full bucket after an owner dies? Is any network wait inside a database transaction? Is the refresh single-flight lock keyed so two distinct scopes cannot share a file, and does the 30 s waiter serve honest stale/partial metadata? Can `watch` starve a foreground submit at concurrency one, or can a pending journal freeze readback of an `outcome_unknown` journal? Events: first baseline silent, partial page never emits a removal, `grade.posted` only with `posted_at`, kill between the cache commit and the state transaction yields replay or `resync_required` and never a silent gap, expired or foreign-generation cursor emits `resync_required` and exits 0, 30-day retention expires rows transactionally and never the file, `cache clear` leaves the state tables alone, no payload carries a token, signed URL, full message body, or DOM text. Check that every existing governor test still passes unchanged and that the §13 `todo` targets hold with a `watch` process running (rerun `cargo xtask bench --runs 3` and `--watch --runs 3`). The worker's final report, for reference:

```

```
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m6c`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M6-c):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M6-c.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M6-c.md`
