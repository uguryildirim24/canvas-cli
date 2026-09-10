# Adversarial review round 7 — docs/SPEC.md v0.7 (review only, do not edit SPEC.md)

You reviewed v0.1–v0.6 (`docs/reviews/spec-v0.*-review.md`). The author rewrote
the spec as v0.7 and answered every round-6 finding in SPEC Appendix E
("Round 6"). Your job now:

1. For each round-6 finding (R6-B01, R6-M01–R6-M02, R6-N01–R6-N02) and each
   earlier item you marked `partially resolved` in round 6: confirm the v0.7
   text actually resolves it, or mark it `still open` / `partially resolved`
   with the exact gap. Check the cited section, not Appendix E.
2. Attack the new material with the same rigor. Priority targets:
   - §12.2 `response_origin` classification (is "Canvas request-id header +
     Canvas error JSON" a sound signal that the application finished the
     request on its synchronous path?), the two-read negative check, the
     `--assume-not-submitted` escape, absence-before-filters ordering.
   - §12.3 initialization order and the fingerprint-only rebind rule.
   - §8 activation flag clearing.
   - Appendix D `submit@1`/`reconcile@1`/`Journal` vs §12.2 text; §14.
3. API claims: mark "unchanged" where nothing changed. The new API assertion
   is the `response_origin = app` signal: check against the pinned Canvas
   error handling (which headers and body shape the application emits on
   rescued errors, and whether a proxy in front of Canvas can emit the same
   shape before the application finished).

Output: `docs/reviews/spec-v0.7-review.md` with a 5-line verdict
(ship / fix-then-ship / rethink), a resolution table for every round-6 ID and
every carried-over ID, and a findings table for new issues (BLOCKER > MAJOR >
MINOR, same columns as before). If no BLOCKER remains, the verdict's first
line must contain the exact words "No blockers remain". Keep new MAJOR
findings to defects that would make an implementer build the wrong thing;
put style, wording, and example nits under MINOR.
Do not edit any other file. Do not write code. When finished reply exactly:
`DONE docs/reviews/spec-v0.7-review.md`
