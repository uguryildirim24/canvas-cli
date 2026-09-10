# Adversarial review round 3 — docs/SPEC.md v0.3 (review only, do not edit SPEC.md)

You reviewed v0.1 (`docs/reviews/spec-v0.1-review.md`) and v0.2
(`docs/reviews/spec-v0.2-review.md`). The author rewrote the spec as v0.3 and
answered every round-1 and round-2 finding in SPEC Appendix E. Your job now:

1. For each round-2 finding (R2-B01–R2-B04, R2-M01–R2-M25, R2-N01–R2-N03) and
   each round-1 item you marked `partially resolved` or `still open` in round 2:
   confirm the v0.3 text actually resolves it, or mark it `still open` /
   `partially resolved` with the exact gap. Do not accept Appendix E's claim;
   check the section it cites.
2. Attack the new material with the same rigor. Priority targets:
   - §12.2 journal, allowlisted response record, readback, reconcile candidate
     rules (can a receipt still bind the wrong attempt? can reconcile pick the
     wrong candidate? is the text candidate rule sound?), startup recovery,
     exit matrix.
   - §12.3 clobber table, manifest ownership, move-on-rename, the exclusive
     install lock across processes, containment with `open_dir_nofollow` and
     retained handles, Windows rules.
   - §8 selection matrix, env-pair offline rule, token validation, keyring
     error mapping, fallback-file protocol.
   - §10 entity/membership split, thin vs full precedence, context-hash
     coverage, generation re-check, pending-journal read hook, `cache clear`.
   - §11 governor numbers, download classification order, upload completion.
   - §7/§14 outcome enum and exit precedence; Appendix D completeness and
     internal consistency with §5 grammar and §12 text.
   - §18 dependency edges and per-round owners.
3. Re-verify any API claim that changed since v0.2 (grading periods wrapper,
   enrollments `grading_period_id`, planner kinds, modules items route,
   upload `201` branch) only where the v0.3 text differs from what you already
   confirmed. Do not repeat confirmations that still hold; say "unchanged".

Output: `docs/reviews/spec-v0.3-review.md` with a 5-line verdict
(ship / fix-then-ship / rethink), a resolution table for every round-2 ID and
every carried-over round-1 ID, and a findings table for new issues
(BLOCKER > MAJOR > MINOR, same columns as before). If the verdict is ship,
say so plainly and list any residual nits the implementer may fix in code.
Do not edit any other file. Do not write code. When finished reply exactly:
`DONE docs/reviews/spec-v0.3-review.md`
