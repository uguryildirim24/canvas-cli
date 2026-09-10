# M1-a — Store core in `crates/canvas-core` (Cursor Auto, lane w2)

Read `docs/SPEC.md` §9 (paths), §10 (cache, state, locks, epochs), §12.2
"Authoritative record" and "Operation ownership" (journal table shape only),
§13 (SQLite thread), §16 row 2 (tests), Appendix A. Implement the store and
identity modules. No network code and no CLI rendering.

## Deliverables
1. `canvas_core::identity`: `IdentityKey` (host slug + port + user id + 8-hex
   digest per §8), `identity.json` read/write/verify (origin, user_id, key,
   created_at, generation uuid), identity lock at
   `<data root>/locks/<key>.lock` (shared for openers, exclusive for removal,
   `fs4`), re-verification after acquiring, and the removal protocol order
   (credentials callback → directory → profiles callback) with the 5 s
   timeout. Paths come from a `Paths` struct built by the caller (`etcetera`
   lives in the CLI crate; core receives concrete paths).
2. `canvas_core::store`: `Store::open(paths, identity)` opening
   `cache.sqlite` and `state.sqlite` (WAL, `busy_timeout 5000`,
   `PRAGMA user_version`, migrations in one transaction, refuse newer schema,
   `BEGIN IMMEDIATE` for writes). One dedicated SQLite thread per process fed
   by a bounded channel; async handle with `call(|conn| ...)`.
3. **Complete v1 schema**, migration 1: every entity table in §10 with
   per-field-group `observed_at` columns (`core`, `detail`, `status`), the
   scoped keys `enrollment_grades(enrollment_id, period)` and
   `course_totals(course_id, mode)`, `membership`, `fetch_log`
   (`epoch_seen`, `contexts`, `window_start`, `window_end`, `stale`, `error`),
   and in `state.sqlite`: `scope_epoch`, `credential` (active source, token
   sha256, validated_at), `submission_journal` (every field listed in §12.2
   "Journal states"), `alias`, `identity` metadata.
4. `Dataset` trait: scope key, TTL, entity kind, `ingest(pages)` with the
   field-group write rule (a source writes only groups it supplies; newer
   `fetched_at` wins; explicit null in a newer source overwrites), membership
   replacement for the exact scope, `fetch_log` write, epoch check at commit
   (abort if the state epoch for the scope advanced since `epoch_seen`).
   Provide a `FakeEntity` dataset for tests; real datasets come in later
   packages.
5. Hit predicate (§10): `complete = 1`, `stale = 0`, `epoch_seen ≥` state
   epoch (prefix scopes match), age within TTL, window containment with
   context hash. `lookup(dataset, scope, now) -> Hit | Stale(row) | Miss`.
6. `bump_epochs(scopes, &mut state_tx)` used inside a journal transition
   transaction; `pending_for_assignment(id) -> bool` read hook that consults
   `submission_journal` states `uploading|uploaded|posting|outcome_unknown`.
7. `cache stats|clear|path` core functions: `clear` = `DELETE` on every cache
   table in one transaction + `VACUUM`, never unlink.
8. Tests (§16 row 2, store part): two processes opening the same identity;
   interrupted multi-page refresh leaves old rows; epoch abort at commit;
   newer schema refused; `cache clear` with a concurrent reader;
   P→Q→cached P scoped values; list/detail field-group precedence with a
   stale full due date and a fresh thin one; `cache clear` cannot reset an
   epoch; identity removal with a waiting opener sees `identity changed`.

## Rules
- Owner files: `crates/canvas-core/src/{identity,store}/**`,
  `crates/canvas-core/src/lib.rs` module lines for those two, and
  `crates/canvas-core/Cargo.toml` dependencies you need (rusqlite bundled,
  fs4, uuid, sha2, jiff, serde, serde_json, thiserror, tokio). Do not touch
  other modules, other crates, `docs/`, or `tasks/`.
- You own the migration list for this round; number it `0001_initial`.
- Do NOT `git commit`. Leave the tree for review.

## Gates (all must pass before you report)
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
```

Finish with `git status --short` and reply exactly: `DONE M1-a`
