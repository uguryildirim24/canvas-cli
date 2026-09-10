# Adversarial review round 5 — docs/SPEC.md v0.5 (review only, do not edit SPEC.md)

You reviewed v0.1–v0.4 (`docs/reviews/spec-v0.*-review.md`). The author rewrote
the spec as v0.5 and answered every round-4 finding in SPEC Appendix E
("Round 4"). Your job now:

1. For each round-4 finding (R4-B01, R4-M01–R4-M11, R4-N01–R4-N03) and each
   earlier item you marked `partially resolved` in round 4: confirm the v0.5
   text actually resolves it, or mark it `still open` / `partially resolved`
   with the exact gap. Check the cited section, not Appendix E.
2. Attack the new material with the same rigor. Priority targets:
   - §12.2 POST classification table, admission lock + partial unique index,
     owner-lock-before-insert, `planned` recovery, receipt in the success
     transaction, `matched` state and `attribution = unproven`, supersession
     and `receipts acknowledge`, text `server_body_sha256` reference.
   - §12.3 manifest in identity storage keyed by `dest_id`, three-phase move
     with `move_sha256`, marker recovery branches, replacement crash window.
   - §10 `field_obs` per-field observations and `Supplied<T>`; pending hook.
   - §8 credential activation protocol and `cleanup_pending`; env binding
     lock; class-B no-network rule; `identity remove <key>` operand.
   - §11 upload completion handoff wording; governor observation rule,
     cost pre-charge, refill bootstrap, probe timer, 350/300 headroom.
   - Appendix D `receipt@1`, `Journal`, `Candidate`, `verify@1`,
     `reconcile@1` vs §12.2 text; §18 M2-b handoff.
3. API claims: mark "unchanged" where nothing changed. Only re-check claims
   whose v0.5 text differs (the 4xx-as-pre-commit-refusal claim is the one
   new API assertion; check it against the pinned submissions controller).

Output: `docs/reviews/spec-v0.5-review.md` with a 5-line verdict
(ship / fix-then-ship / rethink), a resolution table for every round-4 ID and
every carried-over ID, and a findings table for new issues (BLOCKER > MAJOR >
MINOR, same columns as before). If no BLOCKER remains, the verdict's first
line must contain the exact words "No blockers remain". Keep new MAJOR
findings to defects that would make an implementer build the wrong thing;
put style, wording, and example nits under MINOR.
Do not edit any other file. Do not write code. When finished reply exactly:
`DONE docs/reviews/spec-v0.5-review.md`
