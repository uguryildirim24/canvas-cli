# M2-a — Upload/download transport and the submission journal (Cursor Auto, lane w3)

Read `docs/SPEC.md` §11 (Upload, Download, Transfer requests, governor
interaction), §12.2 ("Authoritative record", "Operation ownership and
admission", journal states, steps 7–12 as state transitions, failure
states, owner-absent recovery table, `reconcile` eligibility — the network
part of reconcile is M2-b), §10 "Mutation epochs" and "Pending hook", §9
(journal lock paths), §16 rows 1 (upload/download) and 2 (journal),
Appendix D `Journal`/`Posted`/`Readback`/`Candidate`. Existing code:
`canvas-api` request phases and governor (M0-b), `canvas-core::store`
(`submission_journal` table, partial unique index, `bump_epochs`,
`pending_for_assignment`), `canvas-core::io` bridge and containment
(M3-b-core). Read their public APIs first.

## Deliverables
1. `canvas-api::upload`: `upload_submission_file(course, assignment, meta,
   body: impl AsyncRead) -> FileId` exactly per §11: metadata POST,
   multipart POST to `upload_url` with params in order and `file` last, no
   token, connect 10 s + write-idle 60 s, no total timeout; response
   consumed as the completion handoff (`3xx` same-origin `Location` → API
   GET; `201` with `id` or GET `Location`; anything else
   `UploadIncomplete`). Streamed SHA-256 returned alongside the ID.
2. `canvas-api::download`: `download(url, sink, expected_size, on_progress)
   -> u64` per §11: transfer-phase rules, `Accept-Encoding: identity`,
   classification order (throttle → Canvas-origin 401/403/404 `Denied` →
   storage 403 `StorageExpired`), `Content-Length` and `expected_size`
   checks → `SizeMismatch`.
3. `canvas-core::journal`: journal ID generation; admission lock
   `assignment-<id>.lock` and owner lock `<journal-id>.lock` under
   `<identity dir>/journals/` (`fs4`, `create_new` semantics, never deleted);
   `Journal::create` (owner lock **before** insert, partial unique index as
   the last admission check, admission lock released after publish);
   guarded transitions `UPDATE … WHERE journal_id=? AND state=?` (zero rows
   → error); the state enum and every field in §12.2 "Journal states"
   (`post_status`, `response_kind`, `not_submitted_evidence`, allowlisted
   response record, readback, `server_match`, receipt record,
   `acknowledged_at`); success transition that stores the receipt and bumps
   epochs in one transaction; owner-absent recovery (non-blocking lock
   probe, re-read, recovery table incl. `planned → refused`); read-side
   `owner: live|absent|n/a` probe that never transitions; supersession and
   acknowledge helpers for the pending hook.
4. Allowlisted response record builder from a decoded POST response and
   from a history entry (`evidence = post-response | history-files`), with
   the raw body hashed in memory only. Nothing else from the body persists.
5. Tests (§16): pagination-independent upload tests with `wiremock`
   (params order, `file` last, no bearer, `3xx` handoff, `201` with body,
   `201` empty + `Location`, missing/off-origin `Location`, redirect of the
   multipart POST → `UploadIncomplete`); download: same-origin token,
   off-origin strip, identity encoding, missing `Content-Length` with wrong
   size, storage 403 → `StorageExpired`, Canvas 404 → `Denied`; journal:
   failure injection after every phase incl. kills at every lock/insert
   boundary and in `planned` (spawn a helper subprocess), two simultaneous
   creations for one assignment (admission lock + unique index), a second
   process probing during every active phase (no state change), two
   recoverers, owner-absent recovery per state, success transaction
   atomicity (receipt present after crash), the allowlisted record contains
   no capability-bearing URL.

## Rules
- You own `crates/canvas-api/src/{upload,download}.rs` and
  `crates/canvas-core/src/journal/**`. No CLI code this round. If you need
  a `pub` accessor in `store` or `io`, add the smallest one and note it.
- You are the **migration owner** this round; none is expected. If you
  must add one, number it `0002_*` and say so in your final message.
- Work on branch `lane/w3` in this worktree. Commit as you go. Before
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
Finish with `git status --short` and reply exactly: `DONE M2-a`
