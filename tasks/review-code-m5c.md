# Code review + fix — M5-c on branch lane/w3 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M5-c
on branch `lane/w3` (worktree `/home/user/projects/canvas-cli/.worktrees/w3`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /home/user/projects/canvas-cli/.worktrees/w3`. Read the package brief `tasks/m5c-e2e-suite.md` and the SPEC sections it
   cites. First run `git merge main` (expect a trivial merge; resolve if needed and commit). Then read `git log main..HEAD --stat` and the full diff. Two product defects the worker found in code outside its lane are yours to fix in this review (they block a green tree): (a) `cargo nextest run --all-features` fails one test on main itself because the M5-a bench fixture serves no `/api/v1/announcements` (and check `calendar_events` too), so M4-b's `sync --full` exits 12 inside the bench smoke test: extend the bench-5 fixture and the wiremock routes so the full sync succeeds, and rerun `cargo xtask bench --runs 3`; (b) the `#[ignore]`d `auth_before_local` precedence test: `session.rs` opens the identity store before resolving credentials, so exit 13 wins over exit 3 while SPEC §14 requires the abort order 2, 3, 13, 4, 5, 6, 7: reorder the session setup so credential resolution (exit 3) is evaluated before the store open (exit 13) without changing any other behaviour, un-ignore the test, and make sure every M0-c auth test still passes. If (b) cannot be done safely, leave the test ignored and put it under "Needs a decision" with the exact reason. The worker's final report, for reference:

```
82 +
      83 +#[tokio::test]
      84 +async fn files() {
      85 +    both_modes("files", &["files", &COURSE_ID.to_string()]).awai
         +t;
      86 +}
      87 +
      88 +#[tokio::test]
      89 +async fn modules() {
      90 +    both_modes("modules", &["modules", &COURSE_ID.to_string(), "
         +--items"]).await;
      91 +}
      92 +
      93 +#[tokio::test]
      94 +async fn sync() {
      95 +    both_modes("sync", &["sync"]).await;
      96 +}
  Searched for 9 patterns, read 2 files, listed 3 directories, ran 21 shell
  commands
  ⎿  Referenced file crates/canvas-cli/tests/e2e/harness.rs
  ⎿  Read crates/canvas-cli/tests/e2e/commands.rs (97 lines)
  ⎿  Read crates/canvas-cli/src/output/now.rs (59 lines)
  ⎿  Referenced file crates/canvas-cli/tests/cli.rs
  ⎿  Read crates/canvas-cli/src/output/mod.rs (39 lines)
⏺ Now the full command coverage.
  Searched for 7 patterns, read 4 files, ran 37 shell commands
⏺ Now the raw-output commands and their --json rejection.
  Searched for 9 patterns, read 2 files, listed 1 directory, ran 51 shell
  commands
⏺ Now docs/testing.md.
  Ran 13 shell commands
⏺ Now merging main to see whether M4-b landed.
❯ Merged; round 3 is complete. Next package: read tasks/m5c-e2e-suite.md and im…
  Merged main, searched for 1 pattern, listed 1 directory, ran 17 shell commands
❯ Heads-up: M4-b (announcements, announcement, calendar, ICS, sync --full) is
  now on main, so every v1 command exists. Run git merge main at your next clean
  point and include those commands in the suite; nothing else changes. Still
```
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/rev-m5c`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M5-c):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M5-c.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M5-c.md`
