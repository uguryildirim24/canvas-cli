# M6-c — Shared coordinator, `watch --jsonl`, events, `notify` (Claude Opus, lane w3)

Post-v1 package from the agent-UX design. Read `docs/agent-ux/REPORT.md`
§3.2 (the `Watch` surface row, the `event@1` registry row, the "cursor
expired" exit row), §3.4 (identity leases held by resident consumers, the
identity-removal rule), §3.6 (**all of it**: coordinator, the §9 storage
additions table, priority and waiters, the `event@1` contents, the
cursor/baseline/dedup rules, retention, the latency table), §4 (the M6-c
row and its acceptance column), then `docs/SPEC.md` §7 (`--json` is one
document; streaming is a separate contract), §9, §10 (databases and
locking, identity lock, datasets and TTLs, mutation epochs, pending hook,
request budgets), §11 (governor: watermark, cost pre-charge, refill,
cooldown, header-silence reset), §12.2 (journal states; `outcome_unknown`
must not stop readback), §14, §15, §16, Appendix D. Precedence: REPORT
§3.6 defines the coordinator and events; SPEC §10 and §11 define what a
dataset, a TTL, and a governor sample mean and are unchanged. Existing
code: `canvas-api::governor` (`Governor`, `GovernorConfig`, the request
phases in `request.rs`), `canvas-core::sync` (`refresh_*`, `sync`),
`canvas-core::store` (migrations, `fetch_log`, `bump_epochs`,
`pending_for_assignment`), `canvas-core::journal`, `canvas-core::plan`
(M6-a), `canvas-core::identity` (shared identity lock), the `sync` and
`submit` commands, `crates/canvas-cli/src/output` (registry). Read their
public APIs first. Not in this package: MCP tools (M6-b), `context.*`,
`bridge`, `here`, `follow` (M7), inbox reads (M8-a).

## Deliverables
1. **Cross-process permits and shared governor** (`canvas-core::coord`).
   `canvas-api` stays disk-free (§13): give `Governor` two seams, a
   `Permits` trait for slot acquisition and a `GovernorState` load/store
   hook (`estimate`, `watermark`, `cooldown_until`, `refill`,
   `updated_at`); the in-process defaults keep today's behaviour and every
   existing governor test passes unchanged. Core implements both: an API
   request holds `<identity dir>/locks/api-slot-<n>.lock` (`0 <= n <
   api_concurrency`, `fs4`, `create_new` if absent, never deleted) for its
   duration, so a dead process frees its slot with its descriptor; the
   `governor` row in `state.sqlite` is read before admission and updated
   after each response under `BEGIN IMMEDIATE`, with the §11 rules applied
   to the shared values (a lower sample always applies; a higher one only
   above the watermark; cost is pre-charged; refill is never assumed above
   10/s; cooldown is shared). No network wait happens inside a database
   transaction. Recover a vanished owner without inventing a full bucket:
   a stale `updated_at` past the header-silence rule resets exactly as §11
   says, nothing more. Storage permits stay per process.
2. **Refresh single-flight.** `<identity dir>/locks/refresh-<dataset>-
   <scope>.lock` (canonical filesystem-safe encoding of the scope key; a
   test proves two distinct scopes never map to one file). Every
   `refresh_*` takes it; a second process that finds it held waits at most
   30 s, then re-reads the cache and serves what exists with the honest
   §7 `freshness`/`stale`/`partial` metadata, or the normal miss/error when
   nothing usable exists. Wire this into the existing commands through
   `canvas-core::sync` so CLI, `sync`, and (later) MCP share it.
3. **Priority.** A foreground `submit`/`plan execute` registers interest
   in `state.sqlite` before its first pre-flight request; while interest is
   registered, `watch` admits no new polling request (a running one may
   finish) and holds no slot. At `api_concurrency = 1` the same rule
   bounds the wait to one in-flight request. A pending journal makes
   `watch` skip refreshes but **never** stops readback/reconciliation of an
   `outcome_unknown` journal. Prove bounded priority and eventual polling
   progress by test.
4. **Events** (`state.sqlite`, migration `0003_events`, you are the
   migration owner this round): `events` (`cursor` INTEGER PRIMARY KEY
   autoincrement, `observation_id`, `kind`, `observed_at`, `identity_key`,
   `generation`, `dataset`, `scope`, `entity_key`, `before`/`after`
   allowlisted JSON), `observations` (the outbox: `observation_id` =
   `<dataset>:<scope>:<fetch_log row id>:<fetched_at>`, `state`
   pending|applied, unique), `baselines` (per dataset+scope: the last
   complete membership and the compared fields, as allowlisted JSON), and
   the `governor` and `interest` rows from items 1 and 3. Kinds:
   `assignment.added|changed|removed`, `due.changed`, `grade.changed`,
   `grade.posted` (only when `posted_at` appears non-null), `announcement.new`,
   `missing.new`, `submission.state` (written in the same transaction as
   the journal transition), `inbox.unread_count` (registered in the schema,
   no producer until M8-a), `resync_required`. Rules: the first complete
   observation of a scope sets the baseline and emits nothing; only a
   complete same-scope membership is compared; `removed` means absent from
   that complete membership; a partial or failed page emits no removal;
   payloads are the allowlisted §12.2 records, never full messages, DOM
   text, tokens, or signed URLs. The cache commit and the state
   transaction are separate: the observation row is written before the
   comparison, and cursor, baseline, and events commit together keyed by
   `observation_id` (a re-run after a crash is idempotent). If the
   recorded observation's cache row is gone or overwritten before it was
   applied, emit one `resync_required` for that scope; a restart never
   pretends the gap did not happen. Retention 30 days, expired
   transactionally on each tick; the file is never deleted; `cache clear`
   leaves all of this untouched.
5. **`canvas watch --jsonl [--since CURSOR] [--once]`** (class C): holds
   the shared identity lock for its lifetime (so `identity remove` reports
   busy, §3.4); staggers dataset refreshes by their §10 TTLs with backoff
   after errors (no universal freshness promise, no 60 s guarantee);
   replays events after `--since` at least once, in cursor order, then
   streams new ones as one complete `event@1` document per line; an
   expired cursor or one from another generation emits `resync_required`
   and exits 0; `--once` runs one tick and exits; `--json` is refused
   (exit 2: the stream is its own contract, §7 unchanged); SIGINT closes
   the stream cleanly with the cursor durable. Register `event@1` and
   `watch@1` (the `--once` summary) in the registry with fixtures.
6. **`canvas notify [--since CURSOR] [--stdout]`** (optional desktop
   notifications): consumes events from item 4 (never a second data
   source), deduplicates by cursor, and posts one desktop notification per
   event kind group (assignments, grades, announcements, missing,
   submission). Use `notify-rust` only if it verifies on crates.io with a
   license `cargo deny` accepts and adds no unsafe or network code;
   otherwise `--stdout` is the only backend and you say so. Record the
   dependency and version in your final message for Appendix A.
7. **MCP hookup (last, needs M6-b).** When `canvas mcp` exists on `main`,
   make its `subscriptions/listen` real: an event on a scope invalidates
   the matching `canvas://<identity-key>/<generation>/…` resource and the
   server sends the resource-updated notification; a client's cursor
   follows the same replay rules. If M6-b has not landed when you reach
   this item, finish everything else, run the gates, commit, and reply
   `WAITING M6-b` on its own line instead of the final marker.
8. **Tests** (REPORT §4 M6-c acceptance, each one explicit): combined
   CLI + watch request count never exceeds `api_concurrency` (two
   processes against one wiremock, asserted from the fixture's concurrent
   in-flight counter); owner death releases its slot and cooldown recovers
   without a full bucket; concurrency-one priority and no starvation of
   polling; same-scope single-flight (two processes, one fetch) and the
   30 s waiter fallback (shorten it in tests through config); partial page
   emits no removal; first baseline silent; kill between the cache commit
   and the state transaction, restart, replay or `resync_required`, never
   a silent gap (spawn a helper subprocess as M2-a did); expired cursor →
   `resync_required` + exit 0; retention expiry and cursor dedup; an
   `outcome_unknown` journal does not halt readback; `event@1` and
   `watch@1` snapshots; README/clap parity; `--json` refused on `watch`.
   Add a `cargo xtask bench --watch` sample (one tick against the bench
   fixture, p50/p95) and append it to `docs/bench.md`; §13 `todo` targets
   must still hold with a `watch` process running (measure and record).

## Rules
- You own `crates/canvas-core/src/coord/**`, `crates/canvas-core/src/
  events/**`, the migration `0003_events`, `crates/canvas-cli/src/commands/
  {watch,notify}.rs`, the `Permits`/`GovernorState` seams in `canvas-api`
  (smallest change; no behaviour change for in-process use), the
  `refresh_*` lock wiring, the registry entries `event@1` and `watch@1`,
  and the `bench --watch` extension. The command enum, registry, and
  README are shared with lane w2 (M6-b adds `schema` and `mcp`): add your
  arms and entries, keep theirs when you `git merge main`, and never
  rename or reorder existing ones.
- Work on branch `lane/w3` in this worktree with
  `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/w3`. Commit
  as you go with conventional messages. Before reporting, `git merge main`
  (resolve, rerun gates). Do not push. Do not merge into `main`.
- Do not touch `docs/` (except the `docs/bench.md` append) or `tasks/`.
  Where the report leaves something undefined, choose the reading that
  emits fewer events and never claims a change it did not observe; name
  each choice in your final message.

## Gates
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
cargo xtask bench --runs 3
cargo xtask bench --watch --runs 3
```
Finish with `git status --short` and reply with the marker `DONE M6-c` on
its own line (or `WAITING M6-b` per item 7).
