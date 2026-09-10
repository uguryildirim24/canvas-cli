# Adversarial review of SPEC v0.7

Verdict: **FIX-THEN-SHIP**; one blocker remains in the new application-response classifier (R7-B01).  
Submission: Two empty reads do not repair an unproven completion signal; positive history and explicitly labeled assumption remain useful.  
Resolved: Fingerprint-only rebinding, initialization recovery, credential flag clearing and the prior output/exit gaps are addressed.  
Remaining: Destination identity must be checked before the fresh-registry branch (R7-M01).  
Acceptance: Correct these two behavioral defects; the owner's confirmed Canvas access remains settled.

Reviewed on 2026-09-09 against `docs/SPEC.md` v0.7, SHA-256 `1a6e72bbb573f9135c5e0a3edf752ad8b0806178eeacee9b93722c679f4b25bd`. Line numbers refer to that file. This covers all 5 round-6 IDs and all 17 earlier IDs marked partially resolved in [round 6](spec-v0.6-review.md), checked in the substantive sections rather than accepted from Appendix E.

This is a specification/source review. No code, account writes, submissions, downloads or runtime tests were performed. The new response-origin assertion was checked against pinned Canvas middleware/error handling and primary proxy documentation. The proxy counterexample is a documented configuration possibility, not an observation about Lasell's deployment. “Resolved” confirms the prior contract correction, not implementation acceptance.

## Round-6 resolution table

| ID | Status | Checked v0.7 text | Resolution or remaining gap |
|---|---|---|---|
| R6-B01 | partially resolved | §12.2, L488–503; §16, L672 | Unbranded gateway/transport failures now stay unknown; absence is checked before filters, and explicit assumption is labeled as such. The new header/body predicate can still classify a proxy response as completed application work. R7-B01. |
| R6-M01 | resolved | §12.3, L547–551 | Only an equal root fingerprint permits transparent rebinding, and canonical_path is updated. Different fingerprints are refused even when the old path disappeared. The earlier same-identity copy counterexamples are closed. |
| R6-M02 | resolved | §8, L272; §16, L672 | Activation clears the selected store's cleanup flag in the publication transaction; subsequent cleanup cannot delete that active credential. |
| R6-N01 | resolved | §12.3, L551 | The root-lock/metadata/identity-lock order is explicit; damaged metadata is refused, missing row without a DB is repaired, and orphan manifests are refused. The identity check's placement in the new branches is a separate defect, R7-M01. |
| R6-N02 | resolved | §12.2, L483–503; §14, L649; Appendix D, L800–819 | Durable response/evidence fields, immediate-submit projection, unproven attribution and final-state exits are defined. Never-sent and assumed outcomes are distinguished in the journal/reconcile contract. |

## Earlier carried-over resolution table

| ID | Status | Checked v0.7 text | Resolution or remaining gap |
|---|---|---|---|
| R5-B01 | partially resolved | §12.2, L479, L488–503 | The direct 400-status inference is gone, but the replacement response-shape inference still permits a false definite negative. R7-B01. |
| R5-M02 | resolved | §12.3, L547–551 | Shared-manifest locking, physical-root validation, rename updates and copy refusal now address the carried aliasing defect. |
| R5-M03 | resolved | §8, L272–292 | Persistent disabled state, two cleanup flags, best-effort retries and chosen-store flag clearing complete the carried lifecycle correction. |
| R4-B01 | partially resolved | §12.2, L488–503 | Plain gateway errors no longer imply completion. A branded gateway error can still enter the automatic negative branch; R7-B01. |
| R4-M08 | resolved | §8, L268, L272–292 | Atomic env bindings and serialized credential activation/logout now cover the carried publication and cleanup failures. |
| R3-M07 | resolved | §8, L272–292 | Active-source/hash resolution, disabled logout state and cleanup ordering are coherent for the reviewed cases. |
| R3-M13 | partially resolved | §12.2; §14, L649 | Final-state exit mapping is fixed, but the state can still be assigned from an unsupported application-completion signal. R7-B01. |
| R2-B03 | resolved | §12.3, L547–580 | The carried same-identity manifest aliasing, move and replacement-crash cases are addressed. Cross-identity initialization is the new R7-M01. |
| R2-M03 | resolved | §8, L272–292 | The previously carried credential-store activation/retry/logout defects are corrected, including pending deletion of a newly chosen store. |
| R2-M14 | partially resolved | §12.3, L547–551 | Physical-root provenance is fixed. Destination identity binding is still skipped in the fresh-registry branch; R7-M01. |
| R2-M20 | partially resolved | §12.2; §14 | Verification and final-state exits remain defined. Automatic negative resolution still needs the correction in R7-B01. |
| B04 | resolved | §§8–10 | The carried identity-selection and credential-lifecycle gaps are addressed; destination-specific identity validation is tracked separately in R7-M01. |
| B06 | partially resolved | §12.2 | Ownership, durable receipts and explicit assumption are sound; response branding still cannot prove the originating POST finished. R7-B01. |
| B07 | resolved | §12.3, L547–580 | The carried capability/SQLite placement and same-identity root-copy defects are addressed. The separate identity-mismatch branch is R7-M01. |
| M15 | resolved | §8, L272–292 | Logout disables use before deletion, and activation atomically cancels deletion of its chosen store. |
| M24 | partially resolved | §12.3, L547–580 | Move/copy and clobber handling are corrected, but a different identity can initialize into an already-bound destination on its first run. R7-M01. |
| M29 | partially resolved | §12.2, L488–503; §14, L649 | Exit follows the final state, but response-origin misclassification can still create the wrong definite-failure state. R7-B01. |

## New findings

| Severity | ID | Section | Problem | Evidence | Proposed fix |
|---|---|---|---|---|---|
| BLOCKER | R7-B01 | §12.2, L488–503; §16, L672 | **The header/body predicate is a response-format heuristic, not proof that the originating POST finished.** A proxy-generated error can satisfy it while Canvas is still processing; two empty reads three seconds apart then produce the same unsafe definite-failure/rerun result as before. | The pinned [request-context middleware][request-context] adds X-Request-Context-Id after @app.call returns, so that ordering is real for responses produced through this middleware. Its outgoing header is just the identifier; incoming HMAC verification at L160–181 does not authenticate this response to the CLI. The [global API handler][api-errors] emits the proposed error shapes, but they are not exclusive to it. NGINX can route timeout errors to a custom [error page][proxy-errors], return an arbitrary [body][proxy-body], and add an arbitrary header even on errors with [add_header ... always][proxy-headers]. A configured JSON timeout response containing an errors array and X-Request-Context-Id therefore passes L488. The CLI reads baseline twice; the delayed POST commits afterwards. This is a configuration-level counterexample, not a claim that the default Canvas proxy or Lasell uses that configuration. The current fixture only covers HTML/no-header 504s. | Do not use header/body branding to authorize an automatic negative transition. For a dispatched POST without a decoded successful attempt, retain unknown until positive evidence or the explicit, labeled user assumption; keep never_sent as a separate known case. The existing 30-minute assumption route makes this workable without claiming certainty. If automatic negatives are retained for a controlled deployment, require an independently established completion/read-consistency contract for that deployment, not this generic predicate. Add branded JSON gateway error → two empty reads → later commit, and keep unrecognized origins labeled unverified rather than asserting they came from a gateway. |
| MAJOR | R7-M01 | §12.3, L547–551 | **The new fresh-destination branch skips the identity binding in existing metadata.** A first run under identity B can register and write into a destination already marked for identity A. | dest.json contains identity A's key and UUID. Under identity B, destinations and the manifest path are in B's identity storage, so neither exists yet. L551 takes the no-row/no-DB branch and registers a fresh destination. The different-identity rejection appears only inside Row present, so it is never reached. B can mix its downloads into A's bound destination; on B's next run the newly present row finally takes the mismatch branch and refuses. Fingerprint comparison and the two locks do not fix this omitted check. | Immediately after successfully reading dest.json, validate its identity key against the active identity on every path, before registry insertion or manifest creation. Only then choose fresh/recovery/orphan/existing-root behavior. Add identity-A metadata opened by identity B with no B registry/manifest, expecting exit 8 before any download or registry write. Preserve the corrected same-identity fingerprint and initialization-recovery rules. |

## New API assertion: request header and error shape

| Check | Result |
|---|---|
| Does Canvas emit this request-id header after its normal application call returns? | **Yes, on the checked middleware path.** RequestContext::Generator calls @app.call, then adds X-Request-Context-Id to the returned headers. That supports the intended ordering when the response is actually from this path. [Pinned middleware][request-context]. |
| Is the outgoing marker an authenticated completion statement? | **No such statement is implemented here.** The generator verifies an HMAC for accepting an incoming context ID from another service, but emits the outgoing identifier without a completion signature or a transaction assertion. That incoming protection must not be confused with verification performed by this CLI. [Pinned generator][request-context]. |
| Do rescued Canvas errors have one exclusive JSON shape? | **No.** The global API error handler supplies errors arrays for several exception classes and optionally adds error_report_id. [Global handler][api-errors]. The submission controller's own RecordInvalid rescue directly serializes e.record.errors and adds neither of those wrappers. [Submission rescue][submission-error]. Thus an unrecognized shape also does not prove that a gateway generated it. |
| Can a front proxy produce both markers without the original handler finishing? | **Yes, as a configuration possibility.** NGINX supports custom processing for 500/502/503/504 error pages, arbitrary response text, and arbitrary response headers on errors. Those facilities suffice to produce the tested header plus an errors array independently of Canvas's original POST. [Error handling][proxy-errors], [response text][proxy-body], [response headers][proxy-headers]. No such configuration was observed on the owner's deployment. |
| Do two reads at least 3 seconds apart close that gap? | **No.** If the original POST is still delayed, both can legitimately observe baseline before its later commit. The delay is a polling interval, not an established completion bound. The synchronous Canvas path checked in [round 6](spec-v0.6-review.md) remains valid; it does not establish the source of a proxy-formatted response. |
| Is the explicit assumption escape itself honest? | **Yes at the specified contract level.** It requires an explicit flag after 30 minutes and a current no-new-attempt observation, records assumed evidence, reports residual duplicate risk and never posts. Absence-before-filters ordering is now explicit. This does not turn the assumption into proof, and the spec does not claim it does. |

The blocker is the inference **format resembles Canvas → originating operation completed**, not an assertion that Canvas normally creates the initial attempt asynchronously. The review found no evidence that this resemblance is an exclusive deployment-level guarantee. The smallest safe correction is to retain uncertainty after ambiguous dispatched POSTs, while keeping the already-specified positive matching and explicit assumption workflows.

## Unchanged claims

| Area | Status |
|---|---|
| Synchronous submission transaction/versioning and subsequent history representation | **Unchanged**; carry forward the pinned-source trace in [round 6](spec-v0.6-review.md). This round rechecked error/header production only. |
| Comment length cap and post-commit validation behavior | **Unchanged**; the cap remains and the relevant local rescue was inspected for its response shape. |
| File-ID copying, matched/history-files attribution, text/URL matching and verification | **Unchanged**; the unproven-attribution and server-body-reference contracts remain intact. |
| Assignment eligibility/includes, planner/missing joins, period queries/wrappers and modules/files discovery | **Unchanged API claims**; no repeated endpoint checks. |
| Upload completion, redirects and bearer boundaries | **Unchanged**; retain the earlier protocol review. |
| Composite observations, governor watermark and Rust/keyring/capability/SQLite versions | **Unchanged contracts or dependency claims**; no platform builds, quota experiments or dependency-version refresh. |
| All-day representation and one-day ICS policy | **Unchanged**; retain the previous Canvas/RFC check and explicit v1 scope. |
| Lane ownership and same-round handoff | **Unchanged**; M2-b still accepts against merged M1-c, with shared-file owners and merge order specified. |

Totals: **5 round-6 resolutions + 17 carried-over resolutions; 1 BLOCKER and 1 MAJOR new finding.** No new MINOR findings are necessary. The owner's confirmed Canvas access remains resolved.

[request-context]: https://github.com/instructure/canvas-lms/blob/1c9f0bb8013ed69c4f2efe11fd483025469b7e6c/gems/request_context/lib/request_context/generator.rb#L48-L181
[api-errors]: https://github.com/instructure/canvas-lms/blob/1c9f0bb8013ed69c4f2efe11fd483025469b7e6c/app/controllers/application_controller.rb#L2280-L2324
[submission-error]: https://github.com/instructure/canvas-lms/blob/1c9f0bb8013ed69c4f2efe11fd483025469b7e6c/app/controllers/submissions_controller.rb#L325-L335
[proxy-errors]: https://nginx.org/en/docs/http/ngx_http_core_module.html#error_page
[proxy-body]: https://nginx.org/en/docs/http/ngx_http_rewrite_module.html#return
[proxy-headers]: https://nginx.org/en/docs/http/ngx_http_headers_module.html#add_header

