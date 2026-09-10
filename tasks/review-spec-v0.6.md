# Adversarial review round 6 — docs/SPEC.md v0.6 (review only, do not edit SPEC.md)

You reviewed v0.1–v0.5 (`docs/reviews/spec-v0.*-review.md`). The author rewrote
the spec as v0.6 and answered every round-5 finding in SPEC Appendix E
("Round 5"). Your job now:

1. For each round-5 finding (R5-B01, R5-M01–R5-M04, R5-N01–R5-N03) and each
   earlier item you marked `partially resolved` in round 5: confirm the v0.6
   text actually resolves it, or mark it `still open` / `partially resolved`
   with the exact gap. Check the cited section, not Appendix E.
2. Attack the new material with the same rigor. Priority targets:
   - §12.2 POST classification (nothing is proof of rollback; immediate
     resolution through history; `post_definite`; the "definite answer +
     empty history = not submitted" rule), comment cap, `matched` exits.
   - §12.3 `destinations` table with root fingerprint, moved vs copied
     roots, two-lock install mutex.
   - §8 credential row with `none` and two cleanup flags; logout ordering.
   - §10 composite `entity_key` for `field_obs`.
   - §11 governor watermark.
   - Appendix D `Candidate`, `reconcile@1`, `receipts@1 export`.
3. API claims: mark "unchanged" where nothing changed. The one new API
   assertion is that once an HTTP response to the submission POST has been
   received, the server has finished and `submission_history` is
   authoritative for whether a new attempt exists; check it against the
   pinned controller and model (transaction boundaries, after-commit jobs
   that could create the attempt later).

Output: `docs/reviews/spec-v0.6-review.md` with a 5-line verdict
(ship / fix-then-ship / rethink), a resolution table for every round-5 ID and
every carried-over ID, and a findings table for new issues (BLOCKER > MAJOR >
MINOR, same columns as before). If no BLOCKER remains, the verdict's first
line must contain the exact words "No blockers remain". Keep new MAJOR
findings to defects that would make an implementer build the wrong thing;
put style, wording, and example nits under MINOR.
Do not edit any other file. Do not write code. When finished reply exactly:
`DONE docs/reviews/spec-v0.6-review.md`
