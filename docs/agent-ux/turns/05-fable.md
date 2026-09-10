# Turn 05 — fable

Replies to your 1–7. I accept every correction; where a correction needs a mechanism, I name it so the report can state it.

**On 1. Agreed.** Attachment persists per tab; each new document is re-validated (account, zone, origin) before any text is released; messages carrying a stale document or frame generation are rejected; pending validation returns `state: validating` with metadata only. Cross-origin navigation ends the Chrome grant, so returning needs a new gesture. Account change needs explicit rebinding. Captured text is erased on pause and detach. The 10-minute hidden-tab pause is a configurable preference (`bridge.pause_hidden_after`), not a browser guarantee.

**On 2. Agreed: routing, not sandboxing.** One active attachment per identity. Consumers opt in with the opaque `attachment_id`; the panel shows which consumer holds it and offers "share with another". CLI `here` may select the sole attachment. The MCP resource `canvas://<identity-key>/context` returns `not_attached` until the client passed the ID to `context.attach`; `subscriptions/listen` never attaches. Report wording: "browser text was never in the cache; cache access is not consent to distribute it."

**On 3. Agreed.** Isolated-world content scripts cannot read page globals [S1]. Primary sources are the browser-reported URL (route patterns give `course_id`, `assignment_id`, `topic_id`, `quiz_id`) and the extension-owned `users/self` probe in isolated code with `redirect: "error"`. A MAIN-world `ENV` probe is optional, hint-only, and never supplies identity or approval. Text release rules: revalidate account after navigation or account signals; carry `account.observed_at`; strip capability query strings (`verifier`, `Signature`, `X-Amz-*`, `token`), hidden inputs, and credential-looking fields; bound text and selection sizes (default 64 KiB, `truncated: true`). Page content and agent notes are data, rendered inert in the panel. The probe's network time is outside the cached-context budget.

**On 4. Agreed, with these mechanics.** `prepare` takes the admission lock only for its own pre-flight and releases it; the plan row holds the frozen outbound bytes (text/HTML) and the file hashes; §12.2 streamed-hash verification stays. `execute`: reacquire admission, re-run pre-flight step 1, compare `can_submit`, `allowed_attempts`, `baseline_attempt`, `group_category_id`, `submission_types`, `due_at`, `lock_at`; insert the journal with `plan_id` in the same transaction as the plan's `executed` transition (unique index on `submission_journal.plan_id`). `executed` = journal-linked; the journal state is the server outcome. Lost response, concurrent execute, or replayed acceptance → return that journal. No automatic repost, ever. Expiry is checked at admission only.

Approval handle: `execute` returns `input_required` with `requestState = { plan_id, handle }`, where `handle` is a random 128-bit value stored in the plan row with its own expiry and `consumer`. The retried request must carry the same handle in `inputResponses`; the CLI validates handle, plan digest, identity generation, and consumer, and marks the handle used before dispatch. `decline`/`cancel` invalidate the handle. Panel approvals arrive only over the native-messaging path and carry the same handle shown in the panel. `prepare`, `reconcile` (without `--assume-not-submitted`), and status reads never prompt.

**On 5. Agreed.** Support matrix in the report: `2026-07-28` (primary: `server/discover`, per-request capabilities, `input_required` elicitation) and `2025-11-25` through an explicit adapter [S2]. Server instance bound to one identity generation; resource URIs namespaced `canvas://<identity-key>/…`; results marked private with `ttlMs` ≤ the dataset TTL; §7 `freshness` remains the coverage truth. Tools: `open.url` (read-only, returns URL), `context.follow` (not read-only, navigates within the granted origin, rejects stale generations), `submission.prepare` (`readOnlyHint: false`, local write). Drop the "ten tools" claim; M6-b measures schema tokens and round-trip time per client and publishes the elicitation/version matrix.

**On 6. Agreed, with a concrete coordinator.** Per-identity, cross-process, no daemon:

- Permits: `<identity dir>/locks/api-slot-<n>.lock` for `n < api_concurrency`; an API request holds one `flock` slot for its duration. A process that dies releases its slot with its descriptor.
- Governor state: a `governor` row in `state.sqlite` (`estimate`, `watermark`, `cooldown_until`, `refill`, `updated_at`) read before admission and updated after each response under `BEGIN IMMEDIATE`; the §11 rules apply to the shared values.
- Priority: `watch` checks for a pending journal before each tick and, while one exists, skips refreshes and holds no slots; `submit` never waits behind `watch` because `watch` takes at most one slot.
- Waiters on a refresh lock time out at 30 s, then reread the cache and serve what exists with `stale: true`.

Events: a state transaction writes `(observation_key, cursor, baseline, event)` together; `observation_key` = `(dataset, scope, fetched_at)` makes re-emission idempotent after a restart; the cache commit is separate and precedes it, so a crash between the two re-runs the comparison next tick. First observation sets the baseline, no events. Only complete same-scope memberships compare; `removed` = "no longer in this membership". `grade.posted` requires `posted_at` non-null appearing; otherwise `grade.changed`. Payloads are the allowlisted §12.2 records. Replay is at-least-once, deduplicated by cursor; expired cursors return `resync_required`. The report states that transitions between polls may be unobservable.

**On 7. Agreed.** Group writes, including group discussion replies, stay deferred until specified. Reads never post or mark read. Approved posting does not authorize AI-generated graded work; the report carries the course-policy boundary. M6-c gains coordinator and event-recovery acceptance; M8-a may start after M6-b without the extension; rubric context is extended in `assignment`, not duplicated; M8-b defines per-operation attribution. OAuth stays a distribution prerequisite. No implementation or measurement is claimed.

**Settled list for the report.** Thesis T1–T2; curated MCP catalog N1 with the annotations above; tiers (turn 03) with the guarantee statement; plan and approval contract N2 with your admission and handle rules; attachment lifecycle N3 with your document and routing rules; bridge host, socket, and account probe; zones; `watch` with the coordinator and event rules above; unused-feature ranking; v2+ non-goal changes (discussions and inbox lifted under plans, quizzes and group writes kept); packages M6-a → M8-c in that order; risks: OAuth, Lasell host, host compatibility, bypass by shell or other controllers.

If you have no further corrections, you write `REPORT.md` and I review it.

## Sources

- S1 https://developer.chrome.com/docs/extensions/develop/concepts/content-scripts
- S2 https://modelcontextprotocol.io/specification/2026-07-28/changelog

AGREED
