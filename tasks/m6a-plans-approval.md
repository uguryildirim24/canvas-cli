# M6-a — Operation plans and approval core (Claude Opus, lane w1)

This is the first post-v1 package. It implements the plan-and-approval
layer that the agent-UX design agreed on. Read `docs/agent-ux/REPORT.md`
§3.2 (the `submission.prepare` / `submission.execute` rows and the
`plan@1` registry row; the CLI exit mappings table), §3.5 (**all of it**:
plan contents, states, expiry, the approval record, the handle binding, the
execute rules), §4 (the M6-a row and its acceptance column), then
`docs/SPEC.md` §10 ("Mutation epochs", "Pending hook"), §12.2 (all of it:
pre-flight, journal states, execution, failure states), §14, §15, §16 row
2, Appendix D (`Journal`, `receipt@1`, `submit@1`). Precedence: for the plan
layer (before a journal exists) REPORT §3.5 wins; from the journal insert
onward SPEC §12.2 is unchanged and wins. Existing code: `canvas-core::
journal`, `canvas-core::submit` (pre-flight, freeze, execute), `canvas-core::
receipts`, `canvas-core::store` (migrations), the `submit` command and
`crates/canvas-cli/src/output` (registry). Read their public APIs first.
No MCP, extension, watch, or bridge code in this package (M6-b, M6-c, M7).

## Deliverables
1. Migration (you are the migration owner; one new numbered migration in
   `state.sqlite`): `plans` (`plan_id`, identity key and generation,
   `consumer?`, `course_id`, `assignment_id`, `kind`, the frozen payload:
   text or HTML outbound bytes with `input_sha256` and `sent_sha256`, files
   with name, size, sha256, `url`, `comment`; `baseline_attempt`,
   `baseline_submission_id`; the eligibility and date observations compared
   at execute: `can_submit`, `allowed_attempts`, `extra_attempts`,
   `group_category_id`, `submission_types`, `due_at`, `lock_at`,
   `unlock_at`; `plan_sha256` over the canonical plan document; `state` in
   `prepared | approved | executed | expired | invalidated`; `created_at`,
   `expires_at` = created + 15 min; `approval` record `{channel, at,
   consumer?, plan_sha256}` nullable; `journal_id?`; `invalidated_reason?`),
   `approval_handles` (`handle` 128-bit random, `plan_id`, `consumer?`,
   `expires_at`, `used_at?`), and `submission_journal.plan_id` with a
   **unique** index. `Journal` and `receipt@1` gain `plan_id?` and
   `approval?` (Appendix D nullable convention: legacy rows expose `null`).
2. `canvas-core::plan`: `prepare(target, payload) -> Plan`: pre-flight step 1
   (fresh `GET` with `include[]=submission&include[]=can_submit`), the
   admission lock taken **only** for its own pre-flight and released
   before returning, §12.2 step 5 input freezing (same code path as
   `submit`), the plan row inserted `prepared`; no upload, no post, no lock
   across human consideration. `issue_handle(plan_id, consumer)`,
   `approve(plan_id, handle, channel, consumer)`: validates handle, plan
   digest, identity generation, consumer, expiry; marks the handle used;
   stores the approval record; `decline`/`cancel` invalidate the handle
   and the plan. `execute(plan_id) -> Journal`: reacquire the admission
   lock; rerun pre-flight step 1; compare every observation listed in 1
   (a changed meaningful fact → `invalidated` with the reason, exit 8);
   expired or unapproved → `refused` with `reason: expired | invalidated |
   approval_required`, exit 8, before any upload or post; then **one state
   transaction**: consume the approval, insert the journal row with
   `plan_id` (the unique index is the guard), copy the approval audit into
   the journal, mark the plan `executed`; uploads begin only after it
   commits; steps 8–12 of §12.2 run unchanged. A concurrent execute, a
   restarted host, or a replayed approval returns the existing journal and
   never creates a second attempt for that plan. Expiry is checked at
   admission only; status reads of an executed plan never expire.
3. Refactor the human `canvas submit` onto `prepare → approve(channel
   "tty", or "yes-flag" for `--yes`) → execute`. Its behaviour, exit codes,
   stderr confirmations, and the single `submit@1` envelope stay exactly as
   the existing snapshot tests expect; `yes-flag` is recorded honestly and
   is never shown as an interactive approval. `receipts show`, `receipts
   export`, and the receipt document carry `plan_id` and `approval`.
4. Schema `plan@1` in the registry with a fixture: the frozen plan (payload
   digests and file hashes, never the outbound bytes themselves), state,
   `expires_at`, the nullable approval audit, `journal_id?`. No new
   user-facing command is required in this package; a hidden or
   `#[cfg(test)]` entry point for `prepare`/`execute` is acceptable if the
   tests need a subprocess.
5. Tests (the M6-a acceptance column, each as at least one test): every
   existing §12.2 test still passes unchanged; concurrent execute and a
   replayed approval create exactly one journal (two processes); a kill
   immediately before and immediately after the approval-link transaction
   (subprocess helper) leaves either no journal and an `approved` plan, or a
   journal and an `executed` plan, never anything else; the approval
   channel and digest are present in the journal row and in the exported
   receipt; expired, invalidated, and unapproved plans exit 8 before any
   network write (wiremock sees no upload or post); changed bytes, a
   changed identity generation, and every changed eligibility observation
   are rejected as `invalidated`; no lock file is held while a plan waits
   for approval (probe from a second process); an `outcome_unknown` journal
   is never reposted by any plan path; the plan's exact body digest and
   file hashes are visible through `plan@1`; wrong handle, wrong consumer,
   reused handle, and a handle for another plan are refused.

## Rules
- You own `crates/canvas-core/src/plan/**`, the migration, the `submit`
  refactor, `receipts`/`receipt@1` field additions, and the registry entry.
  Single-lane round: you are the enum, registry, and migration owner.
- Work on branch `lane/w1` in this checkout with
  `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/w1`. Commit
  as you go with conventional messages. Before reporting, `git merge main`
  (resolve, rerun gates). Do not push. Do not merge into `main`.
- Do not touch `docs/` or `tasks/`. If REPORT §3.5 leaves something
  undefined, choose the reading that never sends a request without a
  recorded human approval and name the choice in your final message.

## Gates
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
```
Finish with `git status --short` and reply with the marker `DONE M6-a` on
its own line.
