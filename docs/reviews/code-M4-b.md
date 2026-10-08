# Code review :  M4-b (`announcements`, `announcement`, `calendar`, ICS, `sync --full`), branch `lane/w2`

Reviewer: Claude Opus 5 (high). Base: `e604fcf` (merge of `main` into
`lane/w2`). Package brief: `tasks/m4b-announcements-calendar.md`.
Reviewer brief: `tasks/review-code-m4b.md`. Spec: `docs/SPEC.md` §5, §6,
§7, §9, §10, §12.1, §12.5, §12.6, §13, §14, §15, §16 rows 2-3,
Appendix A, B, D.

## Verdict

**MERGE-AFTER-DECISION.** The two window datasets, the three commands,
the ICS writer glue and the `sync --full` assembly match §10, §12.5 and
§12.6, and all five gates are green after three `review(M4-b):` commits
that fix three defects :  one of them a panic on a user-supplied operand.
One open question remains: §5 names `submissions` among the datasets
`sync` refreshes, and the package refreshes the `assignments` dataset
(which carries `include[]=submission`) instead of the per-assignment
`submission` dataset. That is a scoping decision with a large
request-count consequence, so it is left for Rolf.

## Gate results

Run with `CARGO_TARGET_DIR=<checkout>`
at `beea53d`.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass |
| `cargo nextest run --all-features` | pass :  465 tests run, 465 passed (464 at the base) |
| `cargo deny check` | pass :  advisories ok, bans ok, licenses ok, sources ok |
| `cargo +1.88 check --workspace --all-targets` | pass |

The package adds no dependency and no migration, so Appendix A and the
migration list are unchanged.

## Defects found and fixed

| Sev | File:line | What was wrong | What I changed | Commit |
|---|---|---|---|---|
| Medium | `crates/canvas-cli/src/commands/duration.rs:12`, `:44` | `parse_duration` and `rfc_duration` split `<N><unit>` with `raw.split_at(raw.len() - 1)`, which indexes bytes. An operand whose last character is multi-byte :  `canvas announcements --since 7é`, `canvas calendar --alarm 30€` :  split inside that character and panicked instead of exiting 2 (§14). Reproduced against `str::split_at` directly and confirmed reachable: both call sites run after the session opens, on the raw operand. | Took the unit as a whole `char` through a shared `split_unit`, so a non-ASCII unit falls out through the existing `None` path. Extended the rejection table with `7é`, `7😀` and a bare `é`, and made it assert both parsers. | `9885632` |
| Low | `crates/canvas-cli/src/commands/calendar.rs:280` (before the fix) | `calendar` built its `partial[]` rows by calling the announcements helper and then string-replacing `announcements:` → `calendar_events:` and `Announcements for` → `Calendar for`. For a context that is not a course :  the `user_<id>` context §12.5 always asks for :  the helper had already folded the raw context code into a course scope, so the row came out as `calendar_events:course:user_12345`. That claims a course scope for a user context and disagrees with the `calendar_events:user_12345` that `sync` records for the same denial (`commands/sync.rs`, `denial_partials`). | Gave `denial_scopes` the dataset name and the word its message uses, and built a `<dataset>:course:<id>` scope only for a course context; any other context keeps its own code. §7's `announcements:course:<id>` shape is unchanged. Added a unit test covering both datasets and both context kinds. | `64b199f` |
| Low | `crates/canvas-cli/src/commands/calendar.rs:226` (before the fix) | `calendar` rendered and validated the whole ICS document on every run, even with no `--ics`, only to harvest the one-day warnings :  which the command already computes itself from `EventRow::longer_span_warning`. A `CalendarItem` the writer refuses (a Canvas `html_url` carrying a control character, `IcsError::InvalidValue`) therefore failed a plain table that never asked for a file, and did it as exit 2, the §14 usage code, for operands the student got right. | Moved the `--alarm` grammar check to where the operand is read (`canvas_core::ics::valid_alarm`, now public), which is what made the unconditional render look necessary, and render the text only for `--ics`. A writer refusal is now exit 1. The listing keeps its own warnings, deduplicated in the command. | `beea53d` |

## What I checked and found correct

- **§10 datasets.** Scope keys are `window:<start>..<end>:ctx:<sha256>`;
  the announcements digest covers sorted course ids and the calendar one
  sorted context codes, so the two never collide on one another's
  coverage (`context_window.rs`, tested). Batches are capped at ten. The
  hit predicate goes through `WindowQuery` with the context hash, and a
  differing hash is a miss (`m4b_tests.rs`).
- **§12.6 batch isolation.** A non-throttle `403` isolates the batch one
  context at a time; auth and throttling abort instead
  (`classify_listing_denial`, tested both ways). Coverage stays complete,
  the surviving contexts are stored, and the denied ones are encoded into
  `fetch_log.error` as `contexts_denied:<status>@<context>` with the
  separator characters rejected at the encoder :  so a later cached read
  still reports them. Request counts are asserted exactly (4 for a 12-course
  window with one denial).
- **§12.5 ICS, verified by test rather than by client.** CRLF; folding to
  75 octets first line and 74 plus a leading space on continuations, never
  inside a UTF-8 sequence, with a round-trip unfold assertion; §3.3.11
  escaping; `DTSTART;VALUE=DATE` with no `DTEND` for all-day, including
  west of UTC, across a spring DST change and with equal start and end;
  UTC `DTSTART`/`DTEND` for timed items; no `DURATION` on a point deadline;
  `UID = canvas-<kind>-<id>@<identity-key>`; `VALARM` only on deadlines;
  the longer-span warning. The integration test asserts no line exceeds 75
  octets over the whole document.
- **§7 raw output.** `has_raw_output` covers `calendar --ics -`, and
  `--json` is refused at every flag position (exit 2). `--ics -` emits no
  envelope and sends warnings to stderr; `--ics PATH` keeps the envelope
  and writes the identical text.
- **§5, §6 resolution.** Bare announcement ids are refused (exit 6); a URL
  resolves `/courses/:cid/discussion_topics/:id`; a cross-origin URL exits
  6. `--unread` filters locally on `read_state`, and nothing anywhere in
  the package issues a write verb, so §12.6's "nothing is marked read"
  holds.
- **§12.5 de-duplication.** Planner rows key on `(kind, plannable_id)` and
  a `calendar_event` planner row maps to kind `event`, so the events
  representation overwrites it by calendar event ID and wins for the event
  fields.
- **Appendix D.** `announcements@1`, `announcement@1` and `calendar@1`
  carry every listed field, always present, `null` for unknown; the sorts
  match (`posted_at` desc then id; `(all_day_date ?? start_at ?? due_at)`
  asc then `uid`). Fixtures and the registry snapshot were updated from
  empty placeholders to real rows, and a test asserts the nullable fields
  are serialised rather than omitted.
- **Appendix B.** Both listing paths and the detail path match the table,
  `per_page=100` included.
- **§9.** `ttl_announcements` (15m) and `ttl_calendar` (1h) match the
  config example, and both keys were already in the `config set` allowlist.
- **§15.** No token reaches a path, a cache row or stdout (asserted in the
  fixture). Stored fields are the same allowlisted-field shape the existing
  datasets use; no raw body is written. `partial[]` names courses by code,
  not by URL.
- **§18, README.** The `feat(R4-enum)` step gives every round-4 command its
  own dispatch arm, the `not_implemented` path is gone from the binary
  (grepped: nothing left outside `xtask`), and dropping the README status
  column is now true :  the README/clap parity test still passes with the
  column removed.
- **`docs/ics-clients.md`.** No zero-lag or auto-refresh claim; it says the
  opposite explicitly. The five Apple/Google checks are recorded as not yet
  observed, which the reviewer brief rules is not a defect.

## Needs a decision

1. **Does `sync` refresh the `submission` dataset?** §5 says `sync`
   "refreshes courses, assignments, submissions, missing, the default
   planner window, enrollment grades, and announcements", and the package
   brief repeats "submissions". Every other word in that list is a §10
   dataset name, which reads as the `submission` dataset (scope
   `assignment:<id>`). The package refreshes `assignments` with
   `include[]=submission` and nothing else, and its test pins the base
   `sync` at exactly 7 requests for a one-course fixture.

   Refreshing the `submission` dataset properly costs one request per
   assignment :  for five courses with thirty assignments each that is 150
   requests on every `sync`, an order of magnitude more than the current
   fan-out, and §10's request-budget table says nothing about `sync`. §5
   also gives no rule for scoping it (all assignments? only submitted
   ones? only the planner window?).

   I did not guess. My recommendation: keep the current behaviour and
   amend §5 to say `assignments` carries submission status, or add a
   scoping rule to §12.2/§10 if the per-assignment dataset really must be
   warmed. Either way it is a one-line spec change, not a code fix.

## Notes, not defects

- `announcements --since` with an absurd duration (`100000w`) silently
  clamps the civil-day window to a single day rather than erroring. The
  emitted `window` reports the clamped range, so the output stays honest.
- Timestamps are compared as RFC 3339 strings when sorting announcements.
  That matches chronological order for every whole-second timestamp;
  it would invert two announcements posted in the same second where one
  carries a fractional part. Canvas does not emit those.
- `calendar --ics PATH` writes with `std::fs::write` on the async thread
  rather than through the §13 blocking bridge, and the file gets default
  permissions rather than `0600`. §12.5 sets no mode for the ICS and §15
  names only journals, receipts and exports, so I left both alone.
