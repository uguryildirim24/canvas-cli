# Code review + fix — M5-b on branch lane/w2 (Codex, gpt-6-astra high)

You are the reviewer for this repo. A Cursor worker finished package M5-b
on branch `lane/w2` (worktree `/Users/rolfie/projects/canvas-cli/.worktrees/w2`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /Users/rolfie/projects/canvas-cli/.worktrees/w2`. Read the package brief `tasks/m5b-dist-packaging.md` and the SPEC sections it
   cites. First run `git merge main` (expect a trivial merge; resolve if needed and commit). Then read `git log main..HEAD --stat` and the full diff. Add the brief's sixth gate `dist plan` (cargo-dist 0.32.0) and `cargo xtask dist-assets` determinism to the gate list. This package prepares distribution only: the repository has no remote, tap, or release, so publishing steps are correctly absent; verify the generated formula, workflows, and runbook are correct for the day they are used (targets, archive contents, binary name `canvas`, crate `canvas-lms-cli`, tap `uguryildirim24/homebrew-tap`, macOS direct downloads not advertised). Round 4 (M4-a grades, M4-b announcements/calendar) is still landing on other lanes, so the README command table may legitimately lag those two; list that under "Deferred to R4 merge" rather than as a defect, but every other README claim must match main.
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m5b`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M5-b):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M5-b.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M5-b.md`
