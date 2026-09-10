# Adversarial review round 4 — docs/SPEC.md v0.4 (review only, do not edit SPEC.md)

You reviewed v0.1, v0.2, and v0.3 (`docs/reviews/spec-v0.*-review.md`). The
author rewrote the spec as v0.4 and answered every round-3 finding in SPEC
Appendix E ("Round 3"). Your job now:

1. For each round-3 finding (R3-B01–R3-B02, R3-M01–R3-M16, R3-N01–R3-N03) and
   each earlier item you marked `partially resolved` in round 3: confirm the
   v0.4 text actually resolves it, or mark it `still open` / `partially
   resolved` with the exact gap. Check the cited section, not Appendix E.
2. Attack the new material with the same rigor. Priority targets:
   - §12.2 owner locks and owner-absent recovery (can a read or a second
     `submit` still change a live journal? can two recoverers race? is the
     file-attribution argument sound? is the text/URL `server_match` honest?).
   - §12.3 install mutex, manifest commit ordering, `pending_move_to`
     recovery, uniqueness pass, no-follow open discipline.
   - §10 scoped keys, field groups, hit predicate, epochs in `state.sqlite`
     (crash windows between journal transition and cache work).
   - §8 command classes, selection matrix, active credential source, env
     binding file, validation outcome table, identity key encoding.
   - §11 request phases and redirect rules, governor with issue-sequence
     ordering and refill estimate.
   - §12.5 one-day all-day rule; §14 exit mapping; Appendix D `Journal`,
     `Posted`, `receipts@1`, `reconcile@1`, `verify@1` shapes vs §12.2 text.
   - §18 three-lane rounds: do the dependencies and shared-file owners hold?
3. API claims: mark "unchanged" where nothing changed. Only re-check claims
   whose v0.4 text differs.

Output: `docs/reviews/spec-v0.4-review.md` with a 5-line verdict
(ship / fix-then-ship / rethink), a resolution table for every round-3 ID and
every carried-over ID, and a findings table for new issues (BLOCKER > MAJOR >
MINOR, same columns as before). If no BLOCKER remains, say so in the verdict's
first line in the exact words "No blockers remain" so the author can start
M0-a while smaller items are folded in.
Do not edit any other file. Do not write code. When finished reply exactly:
`DONE docs/reviews/spec-v0.4-review.md`
