# Adversarial review — docs/SPEC.md v0.1 (review only, do not edit SPEC.md)

You are the adversarial reviewer for canvas-cli, a student-only Rust CLI for
Canvas LMS. Your job is to find what is wrong, missing, or unrealistic in
`docs/SPEC.md` BEFORE it is committed. Assume the author is overconfident.

Inputs: `docs/SPEC.md`, `docs/research/r1-canvas-api.md`,
`docs/research/r2-prior-art.md`, `docs/research/r3-rust-stack.md`. You have
live web search: verify claims against https://canvas.instructure.com/doc/api/
and the canvas-lms source on GitHub when the research is not enough.

Attack these areas, in order:
1. API correctness: every endpoint, parameter, and field in §5, §11, §12 and
   Appendix B. Flag anything that does not exist, is teacher-only, or behaves
   differently than described (planner/items filters, missing_submissions,
   submissions/self includes, upload flow, assignment_groups includes,
   enrollments grades fields, files/folders 403 behaviour, announcements
   context_codes limits).
2. Grade math (§12.4): compare with Canvas' real GradeCalculator (drop rules
   algorithm, weighted groups with no graded items, excused, omit_from_final_grade,
   zero-point assignments, grading periods). Point out where the spec's algorithm
   will diverge from Canvas' current_score.
3. Auth and secrets (§8): token resolution order, keyring account naming,
   fallback file, redaction gaps, what happens when the institution blocks tokens.
4. Cache and freshness (§10): TTL policy holes, stale-after-write cases,
   multi-profile mixing, DB migrations, concurrency between two CLI processes,
   ETag claims.
5. Submit and receipts (§12.2): partial-failure paths (upload ok, POST fails),
   attempt counting, text submissions, late/locked logic, what --verify can and
   cannot prove.
6. Download (§12.3): filename collisions, path traversal, hard links, locked
   items, expiring URLs, huge courses, resumability.
7. Output contract (§7), exit codes (§14), identifiers (§6): ambiguity and
   inconsistency between sections.
8. Feasibility: 30 ms startup with rusqlite + keyring, 3-request todo, 100 ms
   cached todo, current_thread runtime with blocking keyring/sqlite calls.
9. Scope: what in v1 should be v2, and what v2 item is actually required for v1
   to be usable by a Lasell student.
10. Work packages (§18): dependencies that make "parallel" packages collide,
    missing acceptance criteria, missing packages.

Output: write `docs/reviews/spec-v0.1-review.md` with:
- A 5-line verdict: ship / fix-then-ship / rethink, and the top 3 risks.
- Findings table ranked BLOCKER > MAJOR > MINOR: id, section, problem, evidence
  (URL or research file line), proposed fix (concrete text or design change).
- A short list of things the spec gets right that must not be lost.
Do not edit any other file. Do not write code. When finished reply exactly:
`DONE docs/reviews/spec-v0.1-review.md`
