# Adversarial review round 8 — docs/SPEC.md v0.8 (review only, do not edit SPEC.md)

You reviewed v0.1–v0.7 (`docs/reviews/spec-v0.*-review.md`). The author rewrote
the spec as v0.8 and answered every round-7 finding in SPEC Appendix E
("Round 7"). Your job now:

1. For each round-7 finding (R7-B01, R7-M01) and each earlier item you
   marked `partially resolved` in round 7: confirm the v0.8
   text actually resolves it, or mark it `still open` / `partially resolved`
   with the exact gap. Check the cited section, not Appendix E.
2. Attack the new material with the same rigor. Priority targets:
   - §12.2: with no automatic negative inference left, is every path out of
     `outcome_unknown` honest (positive `matched`/`server_match`, explicit
     `--assume-not-submitted`, supersession, acknowledge)? Is `never_sent`
     still sound (owner-absent recovery from `uploaded`)?
   - §12.3 initialization order with the identity check first.
   - Appendix D `submit@1`/`reconcile@1`/`Journal` field names vs §12.2.
3. API claims: mark "unchanged" where nothing changed. v0.8 makes no new
   API assertion.

Output: `docs/reviews/spec-v0.8-review.md` with a 5-line verdict
(ship / fix-then-ship / rethink), a resolution table for every round-7 ID and
every carried-over ID, and a findings table for new issues (BLOCKER > MAJOR >
MINOR, same columns as before). If no BLOCKER remains, the verdict's first
line must contain the exact words "No blockers remain". Keep new MAJOR
findings to defects that would make an implementer build the wrong thing;
put style, wording, and example nits under MINOR.
Do not edit any other file. Do not write code. When finished reply exactly:
`DONE docs/reviews/spec-v0.8-review.md`
