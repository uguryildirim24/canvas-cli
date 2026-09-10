# R1 — Canvas LMS REST/GraphQL API survey (research only, no code)

Goal: an authoritative reference of the Canvas LMS (Instructure) API surface that a
STUDENT-facing command-line client needs. Use Google. Primary sources first:
https://canvas.instructure.com/doc/api/ , https://developerdocs.instructure.com/ ,
https://github.com/instructure/canvas-lms , Instructure Community.

Write findings to `docs/research/r1-canvas-api.md` in this repo. Markdown, tables,
exact HTTP method + path + key params + key response fields. Cite a URL per section.
Mark anything you could not verify as UNVERIFIED. Do not write any code files.

Cover, in this order:
1. Auth: manual access tokens (Account > Settings > New Access Token), whether an
   institution can block students from creating tokens, OAuth2 flow for
   third-party apps (developer keys), `Authorization: Bearer`, token expiry.
2. Base URL pattern (`https://<school>.instructure.com/api/v1`). Find Lasell
   University's Canvas host (likely `lasell.instructure.com`) and anything public
   about Lasell's Canvas setup.
3. Pagination: `Link` header (rel=next/last), `per_page` (max 100), `include[]` and
   `state[]` conventions.
4. Rate limits: `X-Rate-Limit-Remaining`, `X-Request-Cost`, throttle bucket, 403
   "Rate Limit Exceeded", recommended concurrency.
5. Endpoints (method, path, params, response fields) for:
   - users/self, users/self/profile, users/self/todo, users/self/upcoming_events,
     users/self/missing_submissions, users/self/activity_stream
   - courses (enrollment_state, include[]=term,total_scores,syllabus_body,
     favorites), users/self/favorites/courses
   - assignments (list per course, single, `bucket` filter, `include[]=submission`,
     due_at, lock_at, points_possible, submission_types, allowed_extensions,
     has_submitted_submissions), assignment_groups (weights, `include[]=assignments`)
   - submissions: GET .../submissions/self, submission_summary, POST submission
     (online_text_entry, online_url, online_upload, media), comments
   - file upload 3-step flow (request → POST to upload URL → confirm), and the
     "submission file upload" variant
   - grades: enrollments `include[]=grades`, computed_current_score vs final,
     grading periods, gradebook history is NOT needed
   - modules + module items (completion state, sequence), module item "mark done"
   - files & folders per course, file download_url semantics (expiring links),
     `files/{id}` , user files
   - announcements (`/announcements?context_codes[]=course_N`), discussion topics
     + entries, posting a reply
   - calendar_events (context_codes, start/end, `type=assignment`), planner API
     (`/planner/items`, planner overrides, planner notes)
   - quizzes (list, status, NOT taking quizzes), new quizzes note
   - conversations (inbox list, read, send)
   - external tools/LTI: note what a CLI cannot do
6. GraphQL: `/api/graphql`, auth, schema explorer at `/graphiql`, what it's better
   for (batching), known limits.
7. Time/format: ISO 8601 UTC, time-zone fields on user/course, `due_at` null rules.
8. Errors: JSON `errors[]` shape, common codes (401/403/404/422), masquerading N/A.
9. Anything a student CLI is commonly blocked from (permissions, feature flags).

Finish by replying in chat with exactly: `DONE docs/research/r1-canvas-api.md`.
