# Code review + fix — M6-b on branch lane/w2 (Claude Opus 5 high)

You are the reviewer for this repo. A Cursor worker finished package M6-b
on branch `lane/w2` (worktree `/home/user/projects/canvas-cli/.worktrees/w2`). Your job is to inspect it against
`docs/SPEC.md` and **fix what is wrong yourself**, then commit your fixes on
the same branch.

1. `cd /home/user/projects/canvas-cli/.worktrees/w2`. Read the package brief `tasks/m6b-agent-adapters.md` and the SPEC sections it
   cites. First run `git merge main` (expect nothing to do; resolve if needed and commit). Then read `git log main..HEAD --stat` and the full diff. Contract: `docs/agent-ux/REPORT.md` §3.2 (all of it) and §3.5 (the approval handle), `docs/SPEC.md` §7 and §14 for envelopes and exits, and the coordinator reading recorded as §19 item 17 (an executed plan replays the journal's `submit@1` with `replayed: true`). Attack in particular: can any MCP tool or argument dispatch a remote write without a consumed approval handle, and does a host that declares no elicitation get a `approval_required` refusal with nothing dispatched (count wiremock requests)? Is the catalog exactly REPORT §3.2 with nothing forbidden reachable (credentials, token reveal, identity administration, arbitrary HTTP or shell, `--yes`, cache clearing, `download --force`, browser actions) — enumerate it yourself, do not trust the allowlist test alone? Do both declared protocol versions handshake and is any other refused? Are resources namespaced by identity **and** generation so a second generation sees nothing? Does every tool's success and domain-error envelope equal the CLI's `--json` for the same fixture (snapshot diff), and do `structuredContent` and the text content carry the same JSON? Is `ttlMs` never above the remaining freshness of the oldest dataset and 0 for anything unresolved? Does `canvas schema` come from the registry and refuse `--json` with exit 2? Does the skill name exactly the catalog and no forbidden flag, and does `docs/agent-hosts.md` claim only what was run (compare with the transcript of what the worker says it ran)? Verify that the worker's host runs left nothing behind: no keychain entry, no host config entry, no files under `~/.config/canvas-cli` or `~/.local/share/canvas-cli` that are not the owner's own. Rerun `cargo xtask bench --mcp --runs 3`. The worker's final report, for reference:

```
- Decline and cancel both invalidate the plan and report exit 11 with code
    cancelled, as canvas submit does for "No". §7 has no cancelled outcome, so
    the outcome stays error.
  - A plan refusal now reports outcome refused. It reported error with exit 8
    before, which §7 does not allow.
  - submission.execute is idempotentHint: true, because a plan admits at most one
    journal. submission.prepare is idempotentHint: false and not read-only. Only
    execute is a remote write, and a test pins that.
  - An executed plan replays from the journal without a client, so a lost
    response is recoverable with the network gone.
  - submission.prepare refuses text: "-": stdin carries the protocol here.
  - replayed is always present on submit@1. Adding a field keeps @1.
  - plan@1 gained a typed schema. Its fixture carries nulls that an inferred
    schema reads as null-only.
  - The forbidden-surface test no longer greps for exec, which matched
    submission.execute. The write surface is pinned by effect instead.
  The host runs created a scratch identity against a local mock and registered
  the server with each host. All of it is removed: no keychain entry, no host
  config entry, and no files under ~/.config/canvas-cli or
  ~/.local/share/canvas-cli.
```
2. Run the gates: `cargo fmt --all --check`, `cargo clippy --all-targets
   --all-features -- -D warnings`, `cargo nextest run --all-features` (or
   `cargo test`), `cargo deny check`, `cargo +1.88 check --workspace
   --all-targets`. Use `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/rev-m6b`.
3. Adversarial review: spec conformance (names, types, error variants, exit
   codes, schemas), correctness under the §16 cases the brief lists, security
   (§15: token handling, redaction, containment, no raw bodies on disk),
   MSRV 1.88, and dependency pins from Appendix A. Missing tests that the
   brief requires count as defects.
4. Fix every defect you find directly in the code. Keep the worker's
   structure unless it is wrong. Commit each logical fix separately with a
   message starting `review(M6-b):`. Do not push. Do not merge.
5. If something cannot be fixed without a spec change, do not guess: leave
   it and list it under "Needs a decision".

Output: `docs/reviews/code-M6-b.md` with: a 3-line verdict (MERGE /
MERGE-AFTER-DECISION / REJECT), the gate results, a table of defects found
(severity, file:line, what was wrong, what you changed, commit hash), and
"Needs a decision" items. Then reply exactly: `DONE docs/reviews/code-M6-b.md`
