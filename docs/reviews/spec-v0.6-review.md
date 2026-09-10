# Adversarial review of SPEC v0.6

Verdict: **FIX-THEN-SHIP**; one blocker remains in the new response-plus-empty-history failure inference (R6-B01).  
Submission: The comment cap and initial unknown state are correct; receipt durability and honest matching remain intact.  
Storage and auth: Root rebinding still accepts copied ownership, and activation can delete its newly selected credential.  
Resolved: Composite observation keys, the governor watermark, replacement classification and the prior payload nits are addressed.  
Acceptance: Correct the three remaining behavioral defects and the bounded integration details below; Canvas access remains settled.

Reviewed on 2026-09-09 against `docs/SPEC.md` v0.6, SHA-256 `648783c569aa8292a6bd869192b6d07925a1febb0a22465e0bcc915400ec49d3`. References below are spec line numbers. This covers all 8 round-5 findings and all 29 earlier IDs marked partially resolved in [round 5](spec-v0.5-review.md), checked against the substantive text rather than Appendix E.

This is a specification/source review. No code, account writes, submissions, downloads or runtime tests were performed. The new HTTP-response/history assertion was checked against the pinned controller, model, versioning and history serialization paths. Unchanged API claims were not rechecked. A resolved row confirms a corrected contract, not implementation acceptance.

## Round-5 resolution table

| ID | Status | Checked v0.6 text | Resolution or remaining gap |
|---|---|---|---|
| R5-B01 | partially resolved | §12.2, L479, L488–508 | The comment cap and initial outcome_unknown classification fix the direct post-commit-400 mistake. Immediate negative resolution reintroduces false certainty for an HTTP gateway response followed by an empty history read; R6-B01. |
| R5-M01 | resolved | §10, L375–377; §16, L672 | field_obs now uses the complete normalized entity key, including period/mode, and updates clocks and values atomically. The cross-period counterexample is covered explicitly. |
| R5-M02 | partially resolved | §12.3, L547–551 | Root fingerprints and the identity-side lock fix independent mutexes for the same manifest. Missing/stale canonical paths still let copied metadata acquire ownership of a different root; R6-M01. Initialization crash handling also needs the bounded clarification in R6-N01. |
| R5-M03 | partially resolved | §8, L272–292 | A persistent none state, two cleanup flags, and nonblocking cleanup retries fix the prior logout/rotation contradictions. Activation must also clear a retained cleanup flag for its newly chosen store; otherwise it deletes the new credential. R6-M02. |
| R5-M04 | resolved | §11, L430; §16, L671 | The nondecreasing watermark survives an older low observation and rejects the intervening stale high sample. Reset and the exact 3-low/1-lower/2-high fixture are specified. |
| R5-N01 | resolved | §12.3, L568–578 | Interrupted replacement now follows the selected comparison policy; equal-size default reruns can redownload. The prior unconditional modified claim is gone. |
| R5-N02 | resolved | §12.2, L503–530; Appendix D, L797–819 | Candidate matching/null behavior, empty text/URL attachment IDs, export payload, matched outcome/exit, readback projection and local timestamp siblings are defined. New immediate-submit output alignment is R6-N02. |
| R5-N03 | resolved | §6, L189; §10, L377; §§12.1, 13; Appendix C | Local resolver misses, field-specific refresh endpoints, per-field terminology and the three database kinds are aligned. |

## Earlier carried-over resolution table

| ID | Status | Checked v0.6 text | Resolution or remaining gap |
|---|---|---|---|
| R4-B01 | partially resolved | §12.2, L488–503 | Errors initially stay unknown, but a gateway response plus a temporarily empty history read can still produce a definite rerun hint. R6-B01. |
| R4-M05 | resolved | §12.3, L568–580 | Move hashing, durable markers and locked recovery remain; replacement crash classification now agrees with optional local hashing. |
| R4-M07 | resolved | §10, L375–377; §11, L431 | Supplied<T>, per-field timestamps and complete composite observation identity now cover both partial payloads and scoped values. |
| R4-M08 | partially resolved | §8, L268, L272–292 | Env-binding atomicity and staged disabled/cleanup states are specified. Retained flags can still delete the newly active store; R6-M02. |
| R4-M11 | resolved | §11, L430 | Bootstrap/probes/headroom remain, and the monotonic watermark closes the identified observation-order regression. |
| R4-N01 | resolved | §7; §12.2, L503–530; Appendix D | Prior digest nullability, candidate, export, receipt/readback and example defects are addressed. Immediate-submit integration is a new minor gap, R6-N02. |
| R3-M01 | resolved | §12.3, L568–580 | The carried move/replacement recovery gap is addressed under the stated optional-verification policy. |
| R3-M04 | resolved | §10, L375–377 | Both values and observation clocks now distinguish enrollment periods and course-total modes. |
| R3-M07 | partially resolved | §8, L272–292 | Selection/validation and logout persistence are defined. Clear the chosen store's pending-deletion flag on activation; R6-M02. |
| R3-M11 | resolved | §11, L430 | The specified governor now covers the carried delayed-sample, bootstrap and sustained cooldown examples. |
| R3-M13 | partially resolved | §12.2, L488–503; §14 | Error context and exit mapping remain, but the new negative-history decision can falsely assert no submission. R6-B01. |
| R3-M14 | resolved | §7; Appendix D, L797–819 | The previously carried Journal/receipt/verification/output defects are corrected; new immediate-submit alignment is R6-N02. |
| R2-B03 | partially resolved | §12.3, L547–580 | Shared manifest locking is fixed; copied metadata can still be accepted as a moved destination when the recorded path disappears. R6-M01. |
| R2-M03 | partially resolved | §8, L272–292 | Durable none and two cleanup flags fix the previous lifecycle model. Retained chosen-store flags remain unsafe on login. R6-M02. |
| R2-M07 | resolved | §10, L373–405 | Exact membership/coverage and complete per-field observation keys address the carried normalization/freshness defects. |
| R2-M14 | partially resolved | §12.3, L547–580 | Containment, planning and move handling are specified; automatic root rebinding still imports ownership on insufficient evidence. R6-M01. |
| R2-M20 | partially resolved | §12.2, L488–535; §14 | Verification references/refusals are defined, but negative POST resolution remains unsound after a gateway response. R6-B01. |
| R2-M22 | resolved | §7; Appendix D | The carried schema/example defects are fixed. R6-N02 concerns integration of the new immediate-resolution path. |
| R2-M25 | resolved | §11, L430 | Retry accounting, admission/probe behavior and the identified stale-sample ordering cases have defined rules. |
| B04 | partially resolved | §§8–10 | Identity selection and binding are coherent; the new credential cleanup flag lifecycle needs R6-M02. |
| B06 | partially resolved | §12.2 | Durable intent, ownership and receipts remain sound. HTTP-response presence still cannot establish the negative outcome used by immediate reconciliation. R6-B01. |
| B07 | partially resolved | §12.3, L547–580 | No-follow transfer access and identity-stored SQLite are retained; different-root ownership can still be imported through the rebind rule. R6-M01. |
| M15 | partially resolved | §8, L272–292 | Logout disables resolution before deletion; activation must cancel any old deletion request for its selected store. R6-M02. |
| M17 | resolved | §10, L375–377 | Complete scoped keys now govern both field values and their clocks; absent payload members cannot freshen either. |
| M24 | partially resolved | §12.3, L547–580 | Normal clobber and interrupted replacement are aligned. Moved-versus-copied root detection still has a false ownership branch. R6-M01. |
| M28 | resolved | §7; Appendix D | The prior bounded payload gaps are fixed. New auto-reconcile output alignment is R6-N02. |
| M29 | partially resolved | §12.2, L488–503; §14 | A server/gateway response is still promoted to false negative certainty through history; R6-B01. |
| M33 | resolved | §11, L430 | The monotonic watermark completes the carried governor correction; this is still a heuristic policy, not a measured server guarantee. |
| N05 | resolved | §§5–8, 18; Appendix D | The carried local-command, terminology, output-example and dependency alignment issues are addressed. |

## New findings

| Severity | ID | Section | Problem | Evidence | Proposed fix |
|---|---|---|---|---|---|
| BLOCKER | R6-B01 | §12.2, L488–505; §16, L672 | **Receiving an HTTP response does not prove Canvas has finished the POST.** A gateway timeout followed by a temporarily empty history read is classified as definitely not submitted, even though the original request can still commit. The rerun hint can produce a duplicate or consume the remaining attempt. | L488 sets post_definite=true for every received HTTP response; L489/L503 then infer failure from no attempt above baseline. Counterexample: Canvas's POST worker is delayed before commit; its gateway returns 504; a separate GET worker reads the old history; the CLI commits uploaded_not_submitted; the original POST then commits. [RFC 9110 §15.6.5][http-504] describes an upstream response timeout, not upstream cancellation. The pinned [model][submit-model] saves the attempt before its normal [controller response][submit-controller], but that ordering does not govern a response generated by a gateway. No deferred attempt-creation job is needed for this counterexample. | Separate response_received from evidence that the originating application operation completed. Keep negative history after gateway/transport-ambiguous failures outcome_unknown; immediate positive history can still produce matched or an honest server_match. Do not promote an empty poll to definite failure without an established operation-completion and authoritative-read contract. Require successful, complete, fresh history evidence and evaluate absence before content/time candidate filters. Preserve known pre-POST refusal separately. Add 504 → empty GET → later POST commit, not only the existing commit → 504 → visible history fixture; a later empty poll must not be advertised as preventing a duplicate rerun. |
| MAJOR | R6-M01 | §12.3, L547–551, L568–578 | **A missing old pathname is mistaken for proof that the destination moved.** The new root fingerprint protects copies only while the recorded original path still names the original directory. It does not establish ownership of files in a different root. | A managed root A has registry fingerprint FA. Copy only its .canvas-cli metadata into unrelated B, then rename A to C. At B, fingerprint FB differs and recorded path A is gone, so L551 calls B moved and imports A's manifest. B's unrelated same-size file at a recorded path can then be replaced under L577 without --force. There is also a direct stale-path variant: opening C after the same-filesystem rename sees the equal fingerprint and simply proceeds, leaving canonical_path=A; a later copy is accepted because A is still missing. The new identity-side mutex correctly serializes access, but serialization does not validate ownership. | Recognize a transparent same-root rename by an equal fingerprint and update canonical_path while locked. Treat a different fingerprint as an unverified root even if the old path disappeared or changed. Refuse/reinitialize with an empty manifest, or require an explicit adoption workflow with sufficient per-file validation; do not silently import ownership. Test rename A→C followed by copied metadata at B, and copied metadata followed by deletion/replacement of A. |
| MAJOR | R6-M02 | §8, L272 | **Activation can delete the credential it just installed.** A cleanup flag retained from a failed logout or previous store switch remains set when that same store is selected for the next login. | Example: logout commits active_source=none and cleanup_keyring=true; keyring deletion fails. A later login successfully writes a new keyring token. Activation step 2 sets active_source=keyring and the other store's cleanup flag, but never clears cleanup_keyring. Step 3 attempts every flagged cleanup and deletes the newly active token. The CLI can report successful login while subsequent resolution fails. The same sequence works with a pending file deletion and a later file activation. | In activation step 2, atomically clear the chosen store's cleanup flag while publishing its source/hash, and retain/set cleanup only for inactive stores. Cleanup must never delete the currently active credential under the same serialized operation. Add login after failed logout, login with both flags set, and switching into a store already marked for cleanup. Keep the new durable none state and best-effort inactive-store cleanup. |
| MINOR | R6-N01 | §12.3, L547–551 | **Destination initialization needs an explicit bootstrap/restart case.** The full two-lock mutex depends on dest_id, which is only read or created inside the described initialization; metadata creation and registry insertion also span a file and a DB transaction. | A new destination has no dest_id file from which to choose the identity-side lock. A crash after create_new writes dest.json but before the destinations insert leaves an existing metadata file with no row, yet the next-run branches assume a row exists for fingerprint comparison. A partial file write is another distinguishable initialization failure. These gaps need a repair/refusal policy, not another ownership-inference rule. | Specify root-lock acquisition, reading/generating dest_id, identity-lock acquisition and revalidation in that order. Define recovery or a clear local-persistence error for missing/partial metadata and a missing registry row. Automatic registration must not attach an existing orphan manifest's ownership to an unverified root. Include the initialization crash boundary in acceptance. |
| MINOR | R6-N02 | §12.2, L483–508; §14, L649; Appendix D, L800, L814–819 | **Align the new immediate-resolution path with durable fields and command output.** The old Candidate/export defects are fixed; this is integration detail introduced by running reconciliation inside submit. | reconcile@1 now has attribution, post_definite, server_match and candidates, while submit@1 lacks them even when its internal resolution returns matched or an informative text/URL result; L503 promises attribution in the envelope. The durable row-field list at L483 omits post_definite although later recovery depends on it and Journal exposes it as nullable. The uploaded_not_submitted meaning at L499 now requires a server error/history check, but owner-absent recovery from uploaded at L470 still means the POST was never sent. §14 also still lists any POST 5xx under exit 9 even though immediate file matching now finishes with exit 0. | Define the submit@1 projection for immediate resolution, preserving its own command schema and the unproven-attribution label. Explicitly persist the corrected response/completion evidence from R6-B01; missing evidence after recovery must remain conservative. Broaden uploaded_not_submitted's description to include the known never-sent branch, and keep nonmatching candidates distinct from server_match. Align §14 with the final resolved state rather than the original HTTP status. |

## New API assertion: source check

The pinned source supports a **narrower synchronous-path conclusion**, not the blanket HTTP-response rule:

| Checked path | What the source establishes |
|---|---|
| Attempt creation | The [submission model's before-save path][attempt-increment] increments attempt when submitted_at changes. The [assignment transaction][submit-model] saves the homework and runs versioning before the transaction ends; the [controller][submit-controller] renders its normal response afterwards. |
| History persistence and serialization | [SimplyVersioned][versioning-hook] installs an after-save callback; [version creation][version-create] writes the version during that save path. [submission_history][history-model] reads persisted versions, deduplicates submission timestamps and falls back to the current model when needed. The [API serializer][history-api] maps those records into submission_history. This is a history representation, not a request-completion token. |
| Work after the attempt transaction | The model performs comment/association work and schedules asset processing after the save transaction. Reviewed [commit callbacks][commit-callbacks] invalidate counts/caches; [queued URL snapshots][websnap] are scheduled after an attempt change, and [word-count work][word-count] updates an existing submission. No deferred creation of the initial v1 file/text/URL attempt was identified in this path. |
| HTTP response versus application completion | A response generated by the actual synchronous controller follows the relevant save/exception path. A gateway-generated 504 need not wait for that path to finish: [RFC 9110][http-504] defines an upstream-response timeout. The new post_definite flag explicitly combines these cases, so the narrower source finding cannot justify L489/L503. |

The blocker does not depend on replica lag or an invented asynchronous Canvas attempt job. A still-running original POST, a gateway response, and an ordinary concurrent history GET are sufficient. No request was sent to the owner's Canvas account, and the gateway sequence is a source/protocol-derived counterexample rather than a reproduced incident.

## Unchanged API and dependency claims

| Area | Status |
|---|---|
| Comment validation limit and post-commit comment failure | **Unchanged API fact**, checked in [round 5](spec-v0.5-review.md); v0.6 adds the correct preflight cap. |
| File IDs, copying and unproven attribution | **Unchanged**; retain [round 4's source check](spec-v0.4-review.md) and the v0.5 matched/history-files distinction. |
| Exact-attempt verification and sanitized text-body reference | **Unchanged**; the reference/unavailable contract remains as resolved. The history implementation was read this round only to assess the new negative-resolution claim. |
| Assignment eligibility/includes; scoped grades and grading-period wrappers; planner/missing joins | **Unchanged API claims**; the new composite observation key is a local storage correction. |
| Files/modules discovery and inline completeness | **Unchanged**; root fingerprints, locks and registry rows are local filesystem contracts. |
| Upload completion handoff, redirects and bearer boundaries | **Unchanged**; no repeat upload-protocol check. |
| Throttle headers and dependency versions | **Unchanged**; the watermark change was checked as a local algorithm. No live quota experiment or platform build was performed. |
| All-day representation and ICS one-day policy | **Unchanged**; retain the previous Canvas/RFC checks and explicit v1 scope. |

Totals: **8 round-5 resolutions + 29 carried-over resolutions; 1 BLOCKER, 2 MAJOR, 2 MINOR new findings.** The owner's confirmed Canvas access remains resolved.

[submit-controller]: https://github.com/instructure/canvas-lms/blob/1c9f0bb8013ed69c4f2efe11fd483025469b7e6c/app/controllers/submissions_controller.rb#L325-L379
[submit-model]: https://github.com/instructure/canvas-lms/blob/1c9f0bb8013ed69c4f2efe11fd483025469b7e6c/app/models/abstract_assignment.rb#L2782-L2842
[attempt-increment]: https://github.com/instructure/canvas-lms/blob/1c9f0bb8013ed69c4f2efe11fd483025469b7e6c/app/models/submission.rb#L1707-L1734
[versioning-hook]: https://github.com/instructure/canvas-lms/blob/1c9f0bb8013ed69c4f2efe11fd483025469b7e6c/gems/plugins/simply_versioned/lib/simply_versioned.rb#L100-L102
[version-create]: https://github.com/instructure/canvas-lms/blob/1c9f0bb8013ed69c4f2efe11fd483025469b7e6c/gems/plugins/simply_versioned/lib/simply_versioned.rb#L250-L274
[history-model]: https://github.com/instructure/canvas-lms/blob/1c9f0bb8013ed69c4f2efe11fd483025469b7e6c/app/models/submission.rb#L1798-L1835
[history-api]: https://github.com/instructure/canvas-lms/blob/1c9f0bb8013ed69c4f2efe11fd483025469b7e6c/lib/api/v1/submission.rb#L83-L116
[commit-callbacks]: https://github.com/instructure/canvas-lms/blob/1c9f0bb8013ed69c4f2efe11fd483025469b7e6c/app/models/submission.rb#L522-L544
[websnap]: https://github.com/instructure/canvas-lms/blob/1c9f0bb8013ed69c4f2efe11fd483025469b7e6c/app/models/submission.rb#L2030-L2034
[word-count]: https://github.com/instructure/canvas-lms/blob/1c9f0bb8013ed69c4f2efe11fd483025469b7e6c/app/models/submission.rb#L3692-L3708
[http-504]: https://www.rfc-editor.org/rfc/rfc9110.html#section-15.6.5

