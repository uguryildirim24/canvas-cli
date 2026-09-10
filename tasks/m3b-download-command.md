# M3-b — `download` command (Cursor Auto, lane w2)

Read `docs/SPEC.md` §5 (`download`, "Behaviour notes", "Command classes"),
§7 ("Streams and confirmations": progress bars), §9 (download paths,
`downloads/` under the identity dir), §10 (database rules; `destinations`
table), §11 (transfer requests, `StorageExpired`), §12.3 (all of it), §14
(outcome mapping and precedence: `mismatch` 10 over `partial` 12), §15,
§16 rows 2–3, Appendix A (`cap-std`, `cap-fs-ext`, `fs4`, `indicatif`),
Appendix D (`download@1`). Existing code: `canvas-core::download`
(M3-b-core: planning, sanitization, uniqueness pass, containment, manifest
DB `dl_0001`, clobber table, three-phase move, marker recovery, transport
trait), `canvas-core::io` (blocking bridge), `canvas-api::download` (M2-a
transport: `download(url, sink, expected_size, on_progress)`), the
discovery view and `folders`/`files`/`modules` datasets (M3-a, lane w2,
your own previous package), `crates/canvas-cli/src/output`. Read their
public APIs first.

## Deliverables
1. Wire `canvas-api::download` into the M3-b-core transport trait: `GET
   /files/:id` for a fresh `url` and `size`, transfer with `expected_size`,
   one URL refresh on `StorageExpired`, restart (never resume) an
   incomplete file, streamed SHA-256 into the manifest row at install.
2. `download <course>|--all-courses [--dest DIR] [--module TEXT] [--file ID
   ...] [--jobs N] [--dry-run] [--force] [--verify]` end to end per §12.3:
   destination initialization in the stated order (root lock, `dest.json`,
   identity check before any branch, identity-side lock, `destinations`
   row and fingerprint cases), marker recovery at the start of every run,
   whole-course path planning independent of filters, transfers outside
   the install mutex with `--jobs` workers (default from config), install
   critical section with the clobber table, moves on rename, `--force`
   never changing containment, `--dry-run` printing the plan with actions,
   sizes, and totals; `--verify` re-hashing local files. Progress: one bar
   per active file and a total (`indicatif`), suppressed by `--quiet` and
   `--json`.
3. Outcome mapping per §12.3 and §14: `failed`, `unavailable`,
   `unsafe_path`, `unresolved_move`, `locked` → `partial`, exit 12;
   `unmanaged` and `modified` → exit 0 with a warning and counts unless a
   higher code applies; `--verify` `mismatch` → exit 10 (precedence over
   12); `dry_run` → exit 0. `skipped_external` counted.
4. Renderer and schema `download@1` with fixtures (per-file `action`,
   `previous_path`, `verify`, totals incl. `bytes`); sort by `path`.
5. Tests (§16, `wiremock` for the API and storage origins, real temp
   directories for the destination): end-to-end containment (`..`,
   absolute, symlinked parent, symlinked final path, symlink swapped
   between inspection and open) and clobber rows on wiremock; a rerun
   downloads zero bytes; `--force` on `unmanaged` and `modified`;
   `--verify` mismatch exit 10; `StorageExpired` refresh once then
   `failed`; `--module` and `--file` filters never change ownership or
   suffixes; `--all-courses`; `--dry-run` shows the plan and writes
   nothing (no `.canvas-cli`, no manifest); identity mismatch on
   `dest.json` exits 8 before any write; orphan manifest and damaged
   `dest.json` exit 13; two competing installers; outcome mapping for every
   action; snapshot tests in human and `--json` mode.

## Rules
- You own the `download` command module and renderer, the transport
  wiring, and any glue in `canvas-core::download`. Lane w3 (M4-b) owns the
  command enum and lane w1 (M4-a) owns the schema registry this round;
  keep your dispatch arm in your own module and `git merge main` when
  Claude tells you the interfaces landed.
- Work on branch `lane/w2` in this worktree. Commit as you go. Before
  reporting, `git merge main` (resolve, rerun gates). Do not push.
- Do not touch `docs/` or `tasks/`.

## Gates
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
```
Finish with `git status --short` and reply exactly: `DONE M3-b`
