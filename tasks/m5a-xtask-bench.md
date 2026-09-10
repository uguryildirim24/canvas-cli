# M5-a — `xtask record|sanitize|bench`, `docs/bench.md` (Cursor Auto, lane w1)

Read `docs/SPEC.md` §11 (redaction list), §13 (performance targets and the
5-course fixture rule), §15 (nothing secret on disk), §16 ("Fixtures"
paragraph; fixtures live in `crates/canvas-api/tests/fixtures/`), §19 item
5 (fixture recording from the owner's account is **not yet approved**: build
the tool, prove it on `wiremock`, do not run it against a real account),
Appendix A (tools row; no benchmark crate is pinned: time the binary from
`xtask` with `std::time::Instant` over repeated runs), Appendix B. Existing
code: `xtask/` (M0-a skeleton), `canvas-api` client and redaction, the
fixture layout M0-b created, every command through R4. Read them first.

## Deliverables
1. `cargo xtask record --host ORIGIN --out DIR [--course ID ...]`: walks
   Appendix B for the identity (token from `CANVAS_TOKEN` or the credential
   store through `canvas-core`), stores each response as
   `<method>-<path-slug>[-page-N].json` with its status and the headers the
   client reads (`Link`, `X-Rate-Limit-Remaining`, `X-Request-Cost`,
   `Date`), never the token, never `Authorization`, never signed storage
   URLs. Refuses to write into the tracked fixture directory without
   `--sanitized` input (below).
2. `cargo xtask sanitize --in DIR --out crates/canvas-api/tests/fixtures/<set>`:
   applies the §11 redaction list plus a deterministic pseudonymizer for
   names, e-mails, avatar URLs, login IDs, SIS IDs, file `url`s and
   `verifier` params, course and user IDs (stable mapping within one set),
   free-text bodies (replaced by a same-length placeholder that keeps
   Markdown structure); emits `MANIFEST.json` (set name, record date,
   endpoint list, redaction version). A second pass over sanitized output is
   a no-op (idempotent).
3. `cargo xtask bench [--fixture SET] [--runs N]`: starts `wiremock` on the
   5-course fixture set (generate a synthetic one under `fixtures/bench-5/`
   if no recorded set exists), primes the cache with `canvas sync`, then
   measures with a release build of `canvas`: cached `todo` first-output
   latency (time to the first byte on stdout) p50/p95, full cached `todo`
   wall time p95, cold start p95 (drop the page cache is not possible
   without root: emulate with a fresh copy of the cache file and a new
   process; document the limitation), each once more while one `download`
   stream is active against the same wiremock. Prints a table and writes
   `docs/bench.md` (targets from §13, measured values, machine, date,
   commit, fixture set, runs). Exit 1 when a target is missed unless
   `--no-fail`.
4. Tests: `record` against `wiremock` (headers kept, token absent from every
   byte written); `sanitize` idempotence, stable ID mapping, redaction of
   every §11 key, `MANIFEST.json`; `bench` smoke run with `--runs 2` in CI
   mode (no target enforcement).

## Rules
- You own `xtask/**`, `crates/canvas-api/tests/fixtures/bench-5/**`, and
  `docs/bench.md` (the one file under `docs/` you may write). Lane w3 owns
  the command enum and the registry this round; you should not need them.
- Work on branch `lane/w1` in this checkout. Commit as you go. Before
  reporting, `git merge main` (resolve, rerun gates). Do not push.
- Do not touch `docs/` (except `docs/bench.md`) or `tasks/`.

## Gates
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
cargo xtask bench --runs 3
```
Finish with `git status --short` and reply exactly: `DONE M5-a`
