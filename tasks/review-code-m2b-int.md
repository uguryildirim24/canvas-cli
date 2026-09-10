# Code review + fix — M2-b-integration on branch lane/w3 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M2-b-integration
on branch `lane/w3` (worktree `/Users/rolfie/projects/canvas-cli/.worktrees/w3`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /Users/rolfie/projects/canvas-cli/.worktrees/w3`. Read the package brief `tasks/m2b-submit-receipts.md` and the SPEC sections it
   cites. First run `git merge main` (expect a trivial merge; resolve if needed and commit). Then read `docs/reviews/code-M2-b.md` (the first review of this branch, verdict MERGE, whose "Deferred to M1-c integration" list this follow-up closes) and the diff since that review's last commit `c6f3258`: `git log c6f3258..HEAD --stat` and `git diff c6f3258..HEAD`. Focus on the integration commits (submit operand resolution, `submission` over the M1-c dataset, todo/submission readers during active phases, the acceptance list of brief part 5–7); the earlier review already covered the rest, so re-verify only what those commits touch plus the full gate run. The worker also reported this observation about code outside its lane; verify it and fix it if it is real:

```
clean.
  - The upload-concurrency assertion drove execute for a fixed 80 ms and expected
    two storage requests; under parallel load it saw zero. It now waits for the
    same invariant with a bound instead of a sleep.
  Two things to flag
  - download::install::tests::pending_move_recovery_branches failed once in ~20
    full runs and I could not reproduce it in isolation (8 tries). It is M3-b
    code I don't own, so I left it alone — worth a look by that lane. Its
    scratch() helper builds directory names from a nanosecond clock plus a
    per-process counter in the shared system temp dir and never cleans them up,
```
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m2b-int`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M2-b-integration):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M2-b-integration.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M2-b-integration.md`
