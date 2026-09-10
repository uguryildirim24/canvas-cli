# Adversarial review of SPEC v0.8

Verdict: **SHIP. No blockers remain.**  
Readiness: The specification is ready for M0-a; no new MAJOR defects were found.  
Submission: Positive evidence, explicit assumption, supersession and acknowledgement preserve the distinction between observation and uncertainty.  
Destination: Identity validation now covers every existing-metadata branch before registration or manifest creation.  
Follow-up: Fold in the one MINOR wording correction below; implementation and platform acceptance remain to be demonstrated.

Reviewed on 2026-09-09 against `docs/SPEC.md` v0.8, SHA-256 `57e765d6d4b7a2ff2e75982337ead35567672988b1613f9675555377e3933f8e`. Line references are to that file. All 2 round-7 IDs and all 9 earlier IDs marked partially resolved in [round 7](spec-v0.7-review.md) are covered below. The substantive sections, schemas and acceptance cases were checked rather than relying on Appendix E.

This is specification review, not implementation approval: no code, account writes, submissions, downloads or runtime tests were performed. No new API assertion is introduced by v0.8. The prior findings are resolved at the contract level.

## Round-7 resolution table

| ID | Status | Checked v0.8 text | Resolution or remaining gap |
|---|---|---|---|
| R7-B01 | resolved | §12.2, L483, L488–508; §14, L649; Appendix D, L800–819 | response_kind is descriptive only. Every dispatched POST without a decoded successful attempt remains unknown unless positive history supports a match or the user explicitly records an assumption. The automatic negative branch and app_error_empty_history evidence are removed. |
| R7-M01 | resolved | §12.3, L551; §16, L672 | Existing dest.json identity is checked before every registry/manifest branch, including no-row/no-DB initialization. The identity-A metadata/identity-B first-run fixture now expects refusal before writes. The phrase about locks needs only R8-N01. |

## Earlier carried-over resolution table

| ID | Status | Checked v0.8 text | Resolution or remaining gap |
|---|---|---|---|
| R6-B01 | resolved | §12.2, L488–503 | HTTP response presence, branding and empty history no longer establish completion or failure. A late commit after empty reconciliation remains possible and is reported honestly. |
| R5-B01 | resolved | §12.2, L479, L488–503 | The comment cap remains; JSON 400 and other errors cannot automatically become a definite not-submitted outcome. |
| R4-B01 | resolved | §12.2, L488–508; §14, L649 | 5xx outcomes remain unknown unless positive history resolves them to an explicitly unproven file match. Empty history does not authorize a definite rerun conclusion. |
| R3-M13 | resolved | §12.2; §14, L649–655 | Phase-specific outcomes and final-state exits no longer depend on unsupported POST-error certainty; durable error context and recovery states remain specified. |
| R2-M14 | resolved | §12.3, L547–580 | Identity validation now precedes fresh initialization as well as existing-root handling. Fingerprint-only rebinding, shared manifest locking, containment and move recovery remain intact. |
| R2-M20 | resolved | §12.2, L488–535; §14 | Verification/refusal mapping is retained, and automatic negative resolution is removed. Explicit assumption remains distinguishable from never-sent evidence. |
| B06 | resolved | §12.2, L457–508 | Durable intent, ownership and receipts now combine with an honest unknown outcome for ambiguous dispatched requests. No response-format shortcut remains. |
| M24 | resolved | §12.3, L547–580 | The remaining cross-identity first-run branch is closed; the earlier clobber, copy/move and interrupted-install corrections remain. |
| M29 | resolved | §12.2, L488–508; §14, L649 | Exit follows the final journal state. Matched history can finish with 0; unresolved/assumed recovery uses 9 without promoting an HTTP error into proof. |

## New findings

| Severity | ID | Section | Problem | Evidence | Proposed fix |
|---|---|---|---|---|---|
| MINOR | R8-N01 | §12.3, L551 | **“Before any lock” conflicts with the numbered initialization order.** The intended identity check is otherwise correctly placed. | Step 1 acquires the root install.lock; step 3 then says the identity check precedes any lock. Step 4 is the identity-side lock that the new check actually precedes. This is a wording conflict, not a recurrence of the fresh-branch identity bug. | Change the phrase to “before the identity-side lock, registry write, manifest creation, or download.” Retain the root lock before reading shared destination metadata. |

## Recovery-path audit

| Path | v0.8 behavior and assessment |
|---|---|
| Error, undecodable success, timeout or dropped connection after dispatch | L488–489 records unknown. Response formatting only describes the response; an empty history cannot establish failure. The branded-gateway counterexample is now an explicit acceptance case at L672. |
| One eligible exact file-set match | L505–507 records matched with history-files evidence, null POST-response hash and attribution=unproven. It records the server-visible attempt without claiming this process created it. Zero or multiple matches stay unknown. |
| Text/URL match or nonmatching newer attempts | L508 retains outcome_unknown even when server_match exists. No receipt is produced. Nonmatches remain candidates and are not described as matches; positive content equality is not request attribution. |
| Explicit assumption | L503 requires the flag, 30-minute age and a current no-new-attempt observation, evaluated before content/time filters. It records not_submitted_evidence=assumed, warns about a possible later commit and never posts. This is a user decision, not a recovered fact. |
| Supersession and acknowledgement | L407 and L537 retire the pending display effect while retaining the old unknown state. Neither manufactures success, failure or a receipt for that operation. |
| Owner-absent uploaded → never_sent | L462–471 requires exclusive ownership recovery and a guarded re-read. L488 requires the durable posting transition before the final submission POST. Therefore a dead owner whose row is still uploaded never reached that POST. Upload requests may already have sent files; never_sent describes the final submission POST. |
| Crash after posting commit but before sending | Recovery yields outcome_unknown. That can conservatively retain uncertainty even when the request never actually left; it does not incorrectly assert a negative. |
| Receipt/export and exits | Confirmed success atomically includes the receipt; matched receipts preserve unproven provenance. Export is refused for unknown/assumed/never-sent journals. L649 follows the final state: matched can exit 0, while unknown or not-submitted recovery exits 9. |

The broad immediate-resolution sentence in L489 is governed by the specific matching rules in L505–508; a merely newer, nonmatching attempt does not become matched. The specification-level guarantees above still require the already-listed failure-injection and two-process acceptance tests in implementation.

## Initialization and schema checks

The initialization sequence now checks the identity of every existing dest.json before taking the identity-side lock or selecting fresh/recovery/orphan/existing-row behavior. A newly created metadata file uses the active identity. Equal fingerprints permit a path update; different fingerprints refuse adoption. The carried cross-identity and copied-root counterexamples no longer enter the wrong manifest.

Journal, submit@1 and reconcile@1 consistently use response_kind with canvas-error/other/none. Journal and reconcile expose only never_sent/assumed for not_submitted_evidence; the removed automatic-negative evidence value is absent from the normative contract. Posted.evidence remains a separate post-response/history-files distinction. submit@1 need not expose assumption evidence because the assumption flag belongs to reconcile. Candidate, attribution, server_match, and final-state exit definitions remain aligned.

## API claims

| Area | Status |
|---|---|
| Response headers/error shapes as completion proof | **Assertion removed.** No replacement API guarantee is asserted; response_kind is descriptive only. |
| Submission save/history semantics, file-ID copying and attribution | **Unchanged.** Carry forward the source checks in [round 6](spec-v0.6-review.md) and [round 7](spec-v0.7-review.md); the revision changes local inference policy. |
| Standard multipart upload versus final submission POST | **Unchanged.** A focused check of the [pinned upload preflight][upload-preflight] confirmed that its URL-fetch auto-submission branch requires params[:url]; that is distinct from the v1 multipart-file path. The never_sent audit above applies to the specified final submission POST. |
| Comment cap, exact-attempt verification and server-body reference | **Unchanged.** The prior validation and integrity contracts remain. |
| Assignment/includes, planner/missing, grading-period queries/wrappers and file/module discovery | **Unchanged.** No new endpoint assertion or broad API recheck. |
| Upload completion/redirects, throttle headers, dependency versions and ICS policy | **Unchanged.** No platform builds, quota experiments, dependency refresh or calendar import was performed. |

Totals: **2 round-7 resolutions + 9 carried-over resolutions, all resolved; 0 BLOCKER, 0 MAJOR, 1 MINOR new finding.** The owner-confirmed Canvas access remains resolved.

[upload-preflight]: https://github.com/instructure/canvas-lms/blob/1c9f0bb8013ed69c4f2efe11fd483025469b7e6c/lib/api/v1/attachment.rb#L374-L451

