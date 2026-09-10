# Code review + fix — M6-a on branch lane/w1 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M6-a
on branch `lane/w1` (worktree `/Users/rolfie/projects/canvas-cli`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /Users/rolfie/projects/canvas-cli`. Read the package brief `tasks/m6a-plans-approval.md` and the SPEC sections it
   cites. First run `git merge main` (expect a trivial merge or nothing to do; resolve if needed and commit). Then read `git log main..HEAD --stat` and the full diff. This is the first post-v1 package: its contract is `docs/agent-ux/REPORT.md` §3.2 (the `submission.prepare`/`submission.execute` rows, the `plan@1` row, the exit mappings table) and §3.5 (all of it) for the plan layer, and `docs/SPEC.md` §12.2 unchanged from the journal insert onward; review against both, in that precedence. Attack in particular: can anything dispatch a remote write without a recorded approval (yes-flag, tty, or a consumed handle)? Can one plan ever admit two journals? Is the 15-minute admission expiry checked only at admission and never after a journal exists? Are the prepared/approved/executed/expired/invalidated transitions guarded (`UPDATE … WHERE state=?`) and is a lost response, a concurrent execute, and a replayed acceptance each answered with the existing journal and never a repost? Does the human `submit` keep exactly its v1 exit codes, stderr and envelope apart from the extra pre-flight GET the worker names? Does `plan_sha256` really pin exact content (sent_sha256 and per-file sha256 re-verified from the stream), and does a missing or foreign-generation plan read as invalidated (exit 8), never approved? The worker's final report, for reference:

```
- Migration STATE_0002: plans, approval_handles, and submission_journal.plan_id
    with a unique index. state.sqlite user_version is now 2. Journal and
    receipt@1 carry plan_id and approval; rows created before plans expose null.
  - canvas-core::plan: prepare → issue_handle → approve / decline / cancel →
    execute. Prepare holds the admission lock for its own pre-flight only.
    Execute revalidates, then journal::create_linked consumes the approval,
    inserts the journal, and marks the plan executed in one transaction; uploads
    start after it commits.
  - canvas submit runs on that path. Exit codes, stderr, and the single submit@1
    envelope are unchanged. --yes records channel yes-flag.
  - plan@1 is in the registry with a fixture. receipts show, receipts export, and
    receipts list carry plan_id and approval.
  - Tests: 18 new plan tests, three of them two-process (kill at the transaction
    boundary, two racing executes plus a replay, an admission probe during an
    approval wait).
  Two behaviour changes worth your attention
  1. Execute reruns pre-flight step 1, so a human submit now makes one extra
     assignment GET. Three M2-b snapshots and two e2e snapshots record the new
     requests.api. REPORT §3.5 requires the revalidation, so I did not try to
     reuse the prepare read.
  2. receipts.json, receipt.json, receipts_show.json and the e2e harness gained
     plan fields and masks for plan_id / plan_sha256. The digest varies per run
     because it covers the identity generation.
  Readings I chose where REPORT §3.5 is silent — each is the one that never sends
  a request without a recorded human approval:
  - Execute compares the frozen observations before the §12.2 eligibility checks,
    so a fact that both changed and now refuses reads as invalidated (needs a
    fresh approval) rather than as a retryable refusal.
  - Execute re-hashes every frozen upload from disk before a journal exists.
    Changed bytes are invalidated, exit 8, with no upload. Text and URL plans
    carry their bytes, so nothing on disk can change what they send.
  - A blocked admission lock waits up to 5 s for this plan's own concurrent
    execute and returns that journal; only a genuinely different submit yields
    the in-progress refusal. One plan never admits two journals.
  - plan_sha256 covers identity, generation, consumer, target, payload digests,
    baseline, observations, and the admission window — not the outbound bytes and
    not the local paths. Content is pinned by sent_sha256 and per-file sha256,
    which §12.2 step 8 re-verifies from the stream, so approving a digest
    approves exact content rather than a path that could later name other bytes.
  - A missing plan reads as invalidated, exit 8, never as approved.
✻ Sautéed for 39m 0s · done 2:40 AM
❯
  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents              /rc
```
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m6a`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M6-a):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M6-a.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M6-a.md`
