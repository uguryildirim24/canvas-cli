# Adversarial review round 2 — docs/SPEC.md v0.2 (review only, do not edit SPEC.md)

You reviewed v0.1 in `docs/reviews/spec-v0.1-review.md`. The author rewrote the
spec as v0.2 and answered every finding in SPEC Appendix E. Your job now:

1. For each v0.1 finding (B01–B08, M01–M37, N01–N06): confirm the v0.2 text
   actually resolves it, or mark it `still open` / `partially resolved` with the
   exact gap. Do not accept Appendix E's claim; check the section it cites.
2. Attack the new material with the same rigor: identity model (§8), journal
   and recovery states (§12.2), containment and skip rules (§12.3), dataset
   table and generations (§10), origin rule and redirect handling (§11),
   Appendix D schemas, exit codes (§14), and the dependency-ordered packages
   (§18). Verify new API claims against the live docs and canvas-lms source
   (same pinned commit is fine): `include[]=can_submit` on assignment show,
   `include[]=current_grading_period_scores` on courses, `type[]`/`state[]`
   on `users/self/enrollments`, `GET /courses/:id/grading_periods` for
   students, planner item kinds and `plannable.assignment_id`, the `201`
   upload completion branch.
3. Verify the keyring 4.2.0 claim: which configuration lets a Rust 1.88 build
   use macOS Keychain, Windows Credential Manager, and Linux Secret Service;
   name the exact crates/features so M0-c does not guess.
4. Verify that `cap-std` (latest 3.x) provides the containment §12.3 relies on,
   including rename within a `Dir`, and note any gap.

Output: `docs/reviews/spec-v0.2-review.md` with a 5-line verdict
(ship / fix-then-ship / rethink), a resolution table for every v0.1 ID, a
findings table for new issues (BLOCKER > MAJOR > MINOR, same columns as
round 1), and the keyring and cap-std answers as short sections.
Do not edit any other file. Do not write code. When finished reply exactly:
`DONE docs/reviews/spec-v0.2-review.md`
