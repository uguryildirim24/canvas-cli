MERGE

Every defect found in this review is fixed and committed on `lane/w2`; all six gates pass on the fixed tree.
`dist plan`, `dist build --artifacts=local`, and `dist build --artifacts=global` were run for real, and the generated Homebrew formula was patched and syntax-checked.
Nothing was pushed, nothing was merged past the required `git merge main`, and no publishing step was performed.

## Scope

Reviewed `tasks/m5b-dist-packaging.md`, its cited SPEC sections (§0, §4, §5, §9, §12.2, §14-§17, Appendices A and D), and the complete worker diff `2b46043..8a3f072`. The required initial `git merge main` completed as `8dd8468` (a trivial merge bringing `tasks/review-code-m5b.md`). Final reviewed tree: `ba4bb13` on `lane/w2`.

This report is the explicit review-task exception to the worker brief's prohibition on writing under `docs/`. No SPEC, task file, dependency pin, migration, or shared R4 enum/registry was changed during the review. `crates/canvas-cli/src/cli.rs` :  the command enum lane w3 owns this round :  is untouched by the whole package.

## Gates

`CARGO_TARGET_DIR=<checkout>`. Toolchain: rustc 1.97.1, cargo-nextest 0.9.143, cargo-dist 0.32.0.

| Gate | Final result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo nextest run --all-features` | PASS: 367 run, 367 passed, 0 skipped (366 before the review's added test) |
| `cargo deny check` | PASS: advisories, bans, licenses, sources |
| `cargo +1.88 check --workspace --all-targets` | PASS, compiler verified as 1.88.0 |
| `dist plan` (brief's sixth gate) | PASS: five targets, each archive listing `canvas`, `man`, `completions` |
| `cargo xtask dist-assets` determinism | PASS: two runs byte-identical; 51 man pages, 5 completion scripts |

The five standard gates also passed on the unmodified worker tree; they did not expose the defects below.

Additional verification beyond the gate list:

- `dist generate --check` :  PASS: the committed `.github/workflows/release.yml` is exactly what dist 0.32.0 generates from `dist-workspace.toml`, so the release job is genuinely reproducible.
- `dist build --artifacts=local --target aarch64-apple-darwin` :  the archive holds `canvas-lms-cli-aarch64-apple-darwin/canvas`, `man/` (51 pages), `completions/` (5 scripts), both licences, `README.md`, `CHANGELOG.md`. The single top-level directory confirms the `cargo binstall` `bin-dir = "{ name }-{ target }/{ bin }{ binary-ext }"` is correct.
- `dist build --artifacts=global` then `cargo xtask dist-formula` on the real generated formula: the anchors matched, the patch applied, and `ruby -c` reports `Syntax OK`. The unit tests' frozen template is a faithful copy of dist 0.32.0's real output.
- `brew audit --strict` and `brew style` could not be run. Homebrew 6.0.22 on this machine aborts before reading any formula (`json-2.21.2 … undefined method 'default_sort_keys_proc='`). `docs/release.md` already records this accurately and defers both to the tap.
- No generated asset contains an absolute path, a user name, or a host name.
- `.TH` headers carry no date, so man pages are reproducible across machines; the clap tree has no `cfg`-gated arguments, so host-generated assets are valid for all five targets.
- Every README row marked `available` was executed against the built binary; none reaches a `not implemented` handler. Every row marked `planned` routes to one.
- The §12.2 receipt caveat in `README.md` is verbatim from `docs/SPEC.md:530`.

## Defects found and fixed

Line numbers identify the original worker tree at `8dd8468`.

| Severity | File:line | What was wrong | What changed | Fix commit |
|---|---|---|---|---|
| Medium | `crates/canvas-cli/build.rs:36,43` | `version@1.commit` reported a commit the binary was not built from. The build script watched `<git-common-dir>/HEAD`, but committing on the current branch rewrites the branch ref and leaves `HEAD` untouched, so cargo never reran it. In this worktree the watched file belonged to the `lane/w1` checkout, so no commit on `lane/w2` could ever move it. Reproduced: build → `git commit` → rebuild still reported the previous commit. | Watch `HEAD` and the ref `HEAD` points at, each located with `git rev-parse --path-format=absolute --git-path` so a linked worktree and a plain checkout both get the right file; add `packed-refs` for a branch with no loose ref. A detached `HEAD` needs only `HEAD`. Without git, or on git older than 2.31, cargo's default file heuristic still applies. Re-reproduced after the fix: the new commit is stamped. | `d1f4dca` |
| Low | `xtask/src/dist_assets.rs:77` | The man page `.TH` source line read `xtask`'s own `CARGO_PKG_VERSION`, not the version of the crate the pages document. Correct only because both inherit `workspace.package.version`. | Added `canvas_cli::dist::VERSION` and read the version from there, so all 51 pages name the `canvas` they describe. | `2e26e78` |
| Low | `xtask/src/dist_assets.rs:89` | `reset_dir` ran `fs::remove_dir_all` on `<out>/man` and `<out>/completions`. `--out` is an arbitrary path from the command line, so `cargo xtask dist-assets --out ~/.local/share` would have deleted the student's whole `man/` tree. | Create the directory, then delete only the assets this task writes: section-1 pages, and completion scripts matched by name against every shell `clap_complete` knows (so dropping a shell from `dist::SHELLS` still clears its script). A stale page from a removed command is still removed. New test `a_rerun_leaves_files_it_did_not_write_alone` points a rerun at a directory holding foreign files. | `d216770` |
| Low | `docs/release.md:196` | The runbook told Rolf that the `dist-formula` tests fail when dist's Homebrew template changes. They patch a frozen copy of the 0.32 template, so they pass whatever dist writes next; only the live `cargo xtask dist-formula` run refuses an unrecognized template. A dist upgrade could have slipped through the step meant to catch it. | Replaced with the step that actually catches it: `dist build --artifacts=global` then `cargo xtask dist-formula`, and an explicit statement that the unit tests cannot detect a template change. | `ba4bb13` |
| Low | `CHANGELOG.md:12` | "The first tag will replace this section with the generated notes" is wrong. `release-plz` inserts each generated release above the `## [Unreleased]` heading and never removes it, so the "0.1.0 is not released yet" paragraph would sit under the released notes indefinitely. | Stated the real behaviour and that the section must be emptied by hand at the first tag. | `ba4bb13` |

## Spec conformance checked and found correct

Recorded because these are the claims a release depends on and each was verified against the artefacts, not just read.

- **§4 names.** Crate `canvas-lms-cli`, binary `canvas`, `[[bin]] name = "canvas"`, clap `name = "canvas"`, formula `canvas-lms-cli.rb` / `class CanvasLmsCli`, tap `uguryildirim24/homebrew-tap`. All five archives are named `canvas-lms-cli-<target>`.
- **§17 targets.** Exactly the five listed, no more, no fewer. Archives carry man pages and completions. `installers = ["homebrew"]` only: no shell or PowerShell installer anywhere, which is stricter than "not on macOS" and matches §17's supported-routes list. `release-plz` owns versions and the changelog.
- **§5 command surface.** One man page per command and subcommand, 51 pages including `canvas.1` and `canvas-submit.1`; clap's implicit `help` gets no page and appears in no `SUBCOMMANDS` section. All five shells generate. `canvas completions <shell>` is byte-identical to the shipped asset, since both call `canvas_cli::dist::write_completions`.
- **§7 raw output.** `completions` is registered in `has_raw_output`, and `--json` on it exits 2 with empty stdout and clap's text error. `version --json` emits one envelope with `profile` and `identity` `null`, as §7 requires for class A.
- **Appendix D `version@1`.** `{ version, commit, target }`, every field present, `commit` serializes as `null` when unstamped. The registry fixture round-trips.
- **§14 exit codes.** `completions` returns 0 on a closed pipe and 1 on a real write failure; nothing in this package introduces a new code.
- **§15 security.** This package handles no token and writes no response body. Nothing in the archives, the man pages, or the completion scripts carries a credential, a path, or a host name.
- **Appendix A pins.** `clap_complete =4.6.9`, `clap_mangen =0.3.3`, `cargo-dist 0.32.0` pinned by `cargo-dist-version` and enforced by the `dist` binary itself. `release-plz 0.3.164` is recorded in `release-plz.toml` and `docs/release.md`; release-plz has no in-config version pin, so documentation is the only mechanism available.
- **MSRV 1.88.** `cargo +1.88 check --workspace --all-targets` passes with the new library target and the rewritten build script.
- **File ownership.** Only the files the brief assigns, plus `crates/canvas-cli/src/main.rs` and `commands/mod.rs` to route the two commands into their new modules. No `docs/` file other than `docs/release.md`, no `tasks/` file.

## Not defects

- **README `grades`, `announcements`, `announcement`, `calendar` marked `planned`.** Deferred to the R4 merge (M4-a and M4-b are still landing on the other lanes), per the review task. Every other row matches `main`: `submit`, `submission*`, and `receipts*` are `planned` because M2-b's routes are still stubs on `main`, and all 32 `available` rows were executed and reach real handlers.
- **`dist plan` warns "A Homebrew tap was specified but the Homebrew publish job is disabled".** Deliberate and documented in `docs/release.md`: dist 0.32 has no hook for extra formula install lines, so the tap commit is the manual `xtask dist-formula` step. The warning is the price of a formula that actually installs the man pages.
- **A local `dist build --artifacts=global` produces a formula with no `sha256` lines.** Expected off CI, and `docs/release.md` says so.
- **The generated formula has no `test do` block.** dist does not emit one and §17 does not ask for one. Worth adding if Rolf ever runs `brew test-bot` (mentioned as optional in the runbook), but it is not required for the tap or for `brew audit --strict`.

## Pre-existing flake, outside this package

`cargo nextest run --all-features` failed once in three consecutive runs of the review's first gate sweep:

```
canvas-core journal::crash_tests::kills_cover_publication_active_phases_and_success_atomicity
panicked at crates/canvas-core/src/journal/crash_tests.rs:229:74:
called `Result::unwrap()` on an `Err` value: InProgress
```

`canvas-core download::install::tests::pending_move_recovery_branches` (`crates/canvas-core/src/download/install.rs:1051`) flaked once in the same campaign. Both are in `canvas-core`, which M5-b does not touch. Reproduced on `main` at `5e78a58` in a throwaway worktree (failed on run 30 of 40) with `crates/canvas-core` byte-identical to the reviewed tree, so it is not introduced here. The mechanism at `crash_tests.rs:229` is a race between reading `before = get_journal(...)` as `None` and the child committing its `planned` row before `kill()`, after which `create` correctly refuses with `InProgress`. It belongs to M2-a (lane w3) and is left for that lane; it does not block this merge. Final gate runs are clean, including a full clean run with no retries.

## Needs a decision

None. Nothing in this package required a SPEC change, and no ambiguity was resolved by guessing.
