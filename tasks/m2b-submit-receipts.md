# M2-b — `submit`, `submission`, `verify`, `reconcile`, `receipts *` (Cursor Auto, lane w3)

Read `docs/SPEC.md` §5 (the `submit`, `submission`, `receipts` lines and
"Command classes"), §6, §7 ("Streams and confirmations"), §10 ("Mutation
epochs", "Pending hook"), §11, §12.2 (all of it, word for word), §12.3
"Containment" (verify downloads into `<identity dir>/tmp/`), §13, §14,
§15, §16 rows 2–3, Appendix B, Appendix D (`Posted`, `Readback`,
`Candidate`, `Journal`, `submit@1`, `submission@1`, `receipt@1`,
`receipts@1`, `verify@1`, `reconcile@1`). Existing code: `canvas-api`
(`upload`, `download`, client, redaction), `canvas-core::journal` (M2-a:
locks, guarded transitions, recovery, response-record builders),
`canvas-core::store` (epochs, pending hook), `canvas-core::io`
(containment), `canvas-core::resolve` and the `submission` dataset (M1-c,
lane w1, lands during this round), `crates/canvas-cli/src/output`. Read
their public APIs first.

## Order of work
M1-c (lane w1) is merged into `main` during this round. Start with
everything that does not need it: parts 1–4 and 6–7 with numeric course
and assignment IDs. When Claude tells you M1-c landed, `git merge main`,
wire `<course> <assignment>` and `<url>` resolution and the `submission`
dataset (part 5), and run the full acceptance on the merged tree before
you report. You merge last this round.

## Deliverables
1. `submit`: pre-flight steps 1–7 exactly per §12.2 (fresh `GET` with
   `include[]=submission&include[]=can_submit`; exit 4 before any journal;
   admission lock; owner-absent recovery of a stale journal for the same
   assignment under the admission lock; group and `external_tool`
   refusals; eligibility incl. the attempts arithmetic; comment length
   exit 2; input freezing for `--file` (upload concurrency 2, streamed
   hash vs frozen hash), `--text` (LF normalization, empty input exit 2,
   HTML transform, `input_sha256` and `sent_sha256`, outbound bytes
   journaled), `--html`, `--url`; plan print and confirmation unless
   `--yes` per §7). Execution steps 8–12: uploads → `posting` → `POST`;
   classification: only a decodable `2xx` with `attempt` is success, every
   other outcome is `outcome_unknown` with `post_status` and
   `response_kind`, **never an automatic negative**; the success
   transaction stores the receipt and bumps the epochs; immediate
   resolution (9b); readback enrichment (11); export with mode `0600`
   (12). Exit codes per the §12.2 failure table.
2. `canvas-core::receipts`: the receipt document (`receipt@1`,
   `attribution`, the `text.server_body_sha256` rule), export and rebuild
   from the journal row, and `receipts list|show|export|acknowledge`
   (class B: never lock, never transition; `export` refuses journals that
   are not `submitted` or `matched`; `--out -` is raw output).
3. `submission reconcile <journal-id> [--assume-not-submitted]`: the
   eligibility table, the history window (`attempt > baseline_attempt`,
   `submitted_at ≥ posting_started_at − 5 min`), file journals (0, 1, or
   2+ entries with the same ID set), text and URL journals (`server_match`
   only, never a match claim), `--assume-not-submitted` only after 30
   minutes and with no visible attempt, idempotent enrichment on
   `submitted`/`matched`; outcomes and exits (`ok` 0, `recovery` 9,
   `refused` 8) with the exact messages §12.2 lists.
4. `submission verify <receipt-id>`: validation before any network call
   (exit 8), history entry selection by `posted.attempt`, attachment
   downloads under containment into `<identity dir>/tmp/` with hash
   comparison, text body digest comparison; outcomes `verified`,
   `verified_body`, `mismatch`, `unavailable`, `refused` with exits 0, 10,
   12, 8.
5. `submission <course> <assignment> [--history]` (class C over the
   `submission` dataset from M1-c; `pending_journals`). After M1-c lands.
6. Renderers and schemas `submit@1`, `submission@1`, `receipt@1`,
   `receipts@1`, `verify@1`, `reconcile@1` with fixtures (M1-c adds the
   registry entries with placeholder fixtures; replace them).
7. Tests (§16, `wiremock` plus a test identity): failure injection after
   every phase; two simultaneous confirmations for one assignment; a
   second process running `todo`, `receipts`, and `reconcile` during every
   active phase; `POST` answered by a Canvas-shaped `500` after the server
   committed → `matched`; JSON `400` after commit → `matched`;
   Canvas-shaped `400` before commit stays `outcome_unknown`; branded JSON
   `504` with a request-id header → empty read → the `POST` commits later →
   a later reconcile finds `matched`; timeout with zero entries stays
   unknown; `--assume-not-submitted` refused before 30 minutes and when
   an attempt is visible; comment over 65,535 characters exits 2; crash
   right after the success transaction (receipt exists, export rebuilds);
   successful `POST` then failed readback (`readback = null`, reconcile
   enriches); another client submits the fresh upload IDs first
   (`matched`, `attribution = unproven`); identical text posted by another
   client during `posting` (server match, stays unknown); file reconcile
   with 0/1/2 matching entries; unknown text journal then a confirmed new
   attempt (superseded, pending clears); `receipts acknowledge`;
   `reconcile` on a `submitted` journal is idempotent; verify: file match,
   mismatch, unavailable, refused URL receipt, text with and without a
   server digest; the receipt is bound to the posted attempt (readback
   never takes fields from another attempt); snapshot tests for every
   schema in human and `--json` mode and for every exit code above.

## Rules
- You own `crates/canvas-core/src/receipts/**`, `canvas-core::journal`
  (extend M2-a's module; keep its lock and guard semantics), and the
  `submit`/`submission`/`receipts` command modules and renderers. M1-c
  (lane w1) owns the command enum and the registry this round and adds
  your variants and schema constants first.
- `reconcile` and `verify` never post. Never delete a lock file. Never
  persist a raw response body.
- Work on branch `lane/w3` in this worktree. Commit as you go. `git merge
  main` when told, and again before reporting (resolve, rerun gates). Do
  not push.
- Do not touch `docs/` or `tasks/`.

## Gates
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
```
Finish with `git status --short` and reply exactly: `DONE M2-b`
