# Code review + fix — M5-a on branch lane/w1 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M5-a
on branch `lane/w1` (worktree `/Users/rolfie/projects/canvas-cli`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /Users/rolfie/projects/canvas-cli`. Read the package brief `tasks/m5a-xtask-bench.md` and the SPEC sections it
   cites. First run `git merge main` (expect a trivial merge; resolve if needed and commit). Then read `git log main..HEAD --stat` and the full diff. Add the brief's sixth gate `cargo xtask bench --runs 3` (the worker added a `.cargo/config.toml` alias) and check `docs/bench.md` against the §13 targets and the run you make yourself. Security focus for this package: `record` must never write a token, an `Authorization` header, or a signed storage URL in any byte of its output, must refuse the tracked fixture directory without sanitized input, and must not have been run against a real account (SPEC §19 item 5 is unresolved; look for real-looking data in any fixture set); `sanitize` must redact every §11 key, pseudonymize stably within a set, and be idempotent; `bench` must not claim a measurement it did not make (the cold-start emulation and the debug-build download load must be stated as limitations).
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-m5a`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M5-a):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M5-a.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M5-a.md`
