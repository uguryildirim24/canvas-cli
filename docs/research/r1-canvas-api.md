# Canvas LMS REST & GraphQL API Survey (Student CLI Reference)

## Executive Summary

This document provides an authoritative, comprehensive reference of the Canvas LMS (Instructure) API surface required for a student-facing command-line interface (CLI) client. The findings are based on primary sources:
* [Canvas LMS REST API Documentation](https://canvas.instructure.com/doc/api/)
* [Instructure Developer Portal](https://developerdocs.instructure.com/)
* [Canvas LMS Open-Source Repository (GitHub)](https://github.com/instructure/canvas-lms)
* [Instructure Community Documentation](https://community.canvaslms.com/)

---

## 1. Authentication & Tokens

### 1.1 Manual Personal Access Tokens
Students can generate personal API access tokens directly through the Canvas web UI:
1. Navigate to **Account** > **Settings** (`/profile/settings` or `/profile`).
2. Scroll to the **Approved Integrations** section.
3. Click the **+ New Access Token** button.
4. Provide a **Purpose** string (e.g. `canvas-cli`) and an optional **Expires** timestamp.
5. Canvas displays the generated plaintext access token **once**. The user must copy and securely store it; it cannot be viewed again.

### 1.2 Institutional Token Restrictions
Institutions have administrative controls to disable or prevent students from creating manual access tokens:
* **Account-Level Setting:** Canvas root account administrators can enable the setting **"Restrict students from creating personal access tokens"** (`settings[restrict_student_access_tokens] = true`) under Account Settings.
* **Role Permissions:** Administrators can revoke the **"Users - Manage Access Tokens"** permission (`manage_developer_keys` / `manage_user_tokens`) for the Student role under Account > Permissions.
* **Impact on Students:** When restricted, the `+ New Access Token` button is completely hidden from the user's settings page for accounts where the user holds only a Student enrollment. Existing tokens created prior to enabling the restriction may remain functional until an administrator deletes them or they expire.
* **CLI Implication:** A student CLI relying solely on manual tokens will fail if the institution enables this restriction. In such environments, OAuth2 developer key authorization is required.

### 1.3 OAuth2 Flow for Third-Party Applications
Canvas implements OAuth2 ([RFC 6749](https://datatracker.ietf.org/doc/html/rfc6749)) with authorization code grant and refresh tokens.

#### Developer Keys
To use OAuth2, an application requires a Developer Key:
* Developer keys are provisioned by institution Root Account Admins under **Admin** > **Developer Keys**.
* Instructure also supports global/multitenant developer keys created by Instructure for certified partner applications.
* Keys provide a `client_id` (numeric ID) and `client_secret` (cryptographic secret), and can be scoped to specific API endpoints.

#### Authentication Flow Endpoints

| Step | HTTP Method & Path | Description & Parameters |
|---|---|---|
| **Step 1: User Authorization** | `GET /login/oauth2/auth` | Redirect user's browser to Canvas authorization page.<br>• `client_id` (string, required): Application client ID.<br>• `response_type=code` (required).<br>• `redirect_uri` (string, required): Registered callback URL, or `urn:ietf:wg:oauth:2.0:oob` for out-of-band / CLI copy-paste.<br>• `state` (string, recommended): Anti-CSRF token.<br>• `scope` (string, optional): Requested scopes (e.g. `/auth/userinfo`). |
| **Step 2: Authorization Code** | Redirect callback | Canvas redirects to `redirect_uri` with `?code=<code>&state=<state>`. If out-of-band (`oob`), Canvas presents a web page displaying the code to the user. |
| **Step 3: Token Exchange** | `POST /login/oauth2/token` | Exchange authorization code for bearer and refresh tokens.<br>• `grant_type=authorization_code`<br>• `client_id` (string, required)<br>• `client_secret` (string, required)<br>• `redirect_uri` (string, required)<br>• `code` (string, required) |
| **Step 4: Token Refresh** | `POST /login/oauth2/token` | Refresh an expired access token using the stored refresh token.<br>• `grant_type=refresh_token`<br>• `client_id` (string, required)<br>• `client_secret` (string, required)<br>• `refresh_token` (string, required) |
| **Step 5: Logout / Revocation** | `DELETE /login/oauth2/token` | Invalidate current session/token.<br>• `expire_sessions=1` (optional): Also invalidate web session. |

#### OAuth2 Token Exchange Response
```json
{
  "access_token": "1/fFAGRNJru1FTz70BzhT3Zg",
  "token_type": "Bearer",
  "user": {
    "id": 42,
    "name": "Student Name"
  },
  "refresh_token": "tIh2YBWGiC0GgGRglT9Ylwv2MnTvy8csfGyfK2PqZmkFYYqYZ0wui4tzI7uBwnN2",
  "expires_in": 3600,
  "canvas_region": "us-east-1"
}
```

### 1.4 Authorization Header Convention
API requests must pass the token via standard HTTP Authorization header:
```http
Authorization: Bearer <ACCESS_TOKEN>
```
* Canvas also permits passing `?access_token=<ACCESS_TOKEN>` as a query parameter or form body parameter, but Instructure explicitly discourages this because tokens appear in server logs, proxy logs, and referer headers.
* Pagination `Link` headers strip `access_token` query parameters for security, requiring clients using query params to manually rewrite all paginated links.

### 1.5 Token Expiry
* **Manual Access Tokens:** By default, do **not** expire unless an expiration date was explicitly chosen by the user at creation time.
* **OAuth2 Access Tokens:** Expire after **1 hour** (`expires_in: 3600` seconds).
* **OAuth2 Refresh Tokens:** Long-lived. They do not expire unless explicitly revoked by the user in User Settings, deleted by an administrator, or deleted via `DELETE /login/oauth2/token`.

**Citations:**
* [Canvas API OAuth2 Overview](https://canvas.instructure.com/doc/api/file.oauth.html)
* [Canvas API OAuth2 Endpoints](https://canvas.instructure.com/doc/api/file.oauth_endpoints.html)
* [Instructure Community: Managing Developer Keys](https://community.canvaslms.com/t5/Admin-Guide/How-do-I-manage-developer-keys-for-an-account/ta-p/249)

---

## 2. Base URL Pattern & Lasell University Deployment

### 2.1 Base URL Pattern
Canvas REST API endpoints adhere to the pattern:
```
https://<school-hostname>/api/v1
```
* Production accounts: `https://<institution>.instructure.com/api/v1`
* Custom vanity domains: `https://courses.<institution>.edu/api/v1`
* Beta testing environment: `https://<institution>.beta.instructure.com/api/v1`
* Test sandbox environment: `https://<institution>.test.instructure.com/api/v1`

### 2.2 Lasell University Canvas Setup
* **Primary Hostname:** `courses.example.test`
* **Custom Vanity Portal:** `courses.lasell.edu` (HTTP 302 redirects to `https://courses.example.test/login/saml`)
* **Medical Science Host:** `medcourses.lasell.edu` (Master of Science in Medical Science programs)
* **Direct Local Login (Bypass SSO):** `https://courses.lasell.edu/login/canvas` (or `https://courses.example.test/login/canvas`)
* **Identity Provider / SSO:** Single Sign-On powered by SAML 2.0 via Microsoft Entra ID (Azure Active Directory), Tenant ID: `3be0a8f7-a09c-4385-9478-93eeaf55f7f0`.
* **Hosting Cluster & Region:** AWS `us-east-1`, Instructure Cloud Cluster `cluster90` (Server: Apache + CloudFront CDN).
* **CSP Frame-Ancestors:** `frame-ancestors 'self' courses.lasell.edu courses.example.test lasell.beta.instructure.com lasell.test.instructure.com;`
* **Help Desk / Support:** Lasell University Technology Help Desk: phone `617-243-2200`, email `helpdesk@lasell.edu`.

**Citations:**
* [Lasell University Canvas Login Directory](https://www.lasell.edu/academics/canvas-login.html)
* [Lasell University Canvas Production Instance](https://courses.example.test)

---

## 3. Pagination & Parameter Conventions

### 3.1 Link Header Standard
Canvas complies with [RFC 5988 / W3C Link Header](http://www.w3.org/Protocols/9707-link-header.html) conventions for all paginated collections.

Response header example:
```http
Link: <https://courses.example.test/api/v1/courses/101/assignments?page=1&per_page=50>; rel="current",
      <https://courses.example.test/api/v1/courses/101/assignments?page=2&per_page=50>; rel="next",
      <https://courses.example.test/api/v1/courses/101/assignments?page=1&per_page=50>; rel="first",
      <https://courses.example.test/api/v1/courses/101/assignments?page=5&per_page=50>; rel="last"
```

#### Key Rules for Clients
1. **Opaque URLs:** Clients must treat URLs in the `Link` header as opaque strings. Do not construct page URLs manually by appending `&page=N`.
2. **Case-Insensitive Parsing:** HTTP/2 and HTTP/1.1 header keys are case-insensitive. Parse `Link` or `link`.
3. **Relation Attributes (`rel`):**
   * `current`: Current page URL.
   * `next`: Next page URL. Omitted on the final page.
   * `prev`: Previous page URL. Omitted on the first page.
   * `first`: First page URL.
   * `last`: Last page URL. Canvas **omits** `rel="last"` if computing the total entry count is too computationally expensive.
4. **Header Size Truncation:** Canvas limits the total generated `Link` header size to 6 KB (Apache max header buffer is 8 KB). If query parameters are excessively long, Canvas prioritizes relations in the following order: `next`, `last`, `prev`, `current`, `first`.

### 3.2 `per_page` Limits
* **Default:** 10 items per page.
* **Maximum:** 100 items per page (`Api::MAX_PER_PAGE = 100` in Canvas source code).
* **Clamping:** Canvas clamps any requested `per_page > 100` down to `100` without returning an error (`per_page_requested.to_i.clamp(1, max)`).

### 3.3 Query Parameter Conventions: `include[]` and `state[]`
Canvas is built on Ruby on Rails and relies on standard Rails rack parameter conventions:
* **Array Parameters:** Use square bracket notation `param[]=<val1>&param[]=<val2>`:
  * `include[]=term&include[]=total_scores&include[]=syllabus_body`
  * `state[]=active&state[]=completed`
  * `context_codes[]=course_101&context_codes[]=course_102`
* *Note:* Some endpoints accept comma-separated strings (e.g. `include=term,total_scores`), but `include[]` is the officially documented standard and is universally supported across all controllers.
* **Compound Documents:** When side-loading related data with `include[]`, responses may return compound documents with a root `"meta": {"primaryCollection": "..."}` object alongside normalized collections.

**Citations:**
* [Canvas API Pagination Documentation](https://canvas.instructure.com/doc/api/file.pagination.html)
* [Canvas LMS Pagination Implementation (`lib/api.rb`)](https://github.com/instructure/canvas-lms/blob/master/lib/api.rb)
* [Canvas API Compound Documents](https://canvas.instructure.com/doc/api/file.compound_documents.html)

---

## 4. Rate Limits & Throttling

### 4.1 Throttling Architecture: Leaky Bucket
Canvas enforces API throttling via custom Rack middleware (`app/middleware/request_throttle.rb`) backed by Redis and server-side Lua scripts. It operates on a **leaky bucket** algorithm:
* Each client request incurs a computed **cost** that is added to the client's bucket.
* The bucket leaks (replenishes capacity) continuously at a constant rate.
* If a request causes the bucket count to reach or exceed the **High Water Mark (HWM)**, the bucket is full, and Canvas rejects the request.

### 4.2 Throttling Headers
Every API response contains throttle telemetry headers:

| Header | Format | Description |
|---|---|---|
| `X-Request-Cost` | Floating-point (e.g. `0.0245`) | Cost of the completed request deducted from user's quota. |
| `X-Rate-Limit-Remaining` | Floating-point (e.g. `700.0`, `642.1`) | Remaining quota before requests will be rejected. |

### 4.3 Default Bucket Constants (from Canvas Source Code)

| Parameter | Default Value | Description |
|---|---|---|
| **High Water Mark (`hwm`)** | `600.0` (or `700.0` on cloud clusters) | Level at which bucket is considered full and requests are rejected. |
| **Maximum Capacity (`maximum`)** | `800.0` | Maximum limit the bucket can hold. |
| **Outflow / Replenish Rate (`outflow`)** | `10.0` points/second | Rate at which the bucket leaks/replenishes back to full capacity. |
| **Up-front Cost (`up_front_cost`)** | `50.0` points/request | Temporary penalty held against bucket while request is processing. |

### 4.4 Cost Calculation Formula
Request cost is computed at request completion:
$$\text{Cost} = (\text{user\_cpu\_seconds} \times 1.0) + (\text{db\_runtime\_seconds} \times 1.0) + \text{extra\_cost}$$
* Simple, fast cached endpoints consume tiny costs (e.g. `0.01` to `0.05`).
* Heavy endpoints (e.g. listing all assignment submissions across all sections) consume high costs (e.g. `2.0` to `5.0+`).

### 4.5 Throttling Status Code: 403 Forbidden vs 429
When throttled, Canvas returns:
* **Default Status Code:** **`HTTP 403 Forbidden`** with body:
  ```text
  403 Forbidden (Rate Limit Exceeded)
  ```
  *(Note: Canvas can return `429 Too Many Requests` if the institution configures `request_throttle.send_429_response = true`, but default installs and production clusters traditionally emit `403 Forbidden`).*
* **Response Header:** `X-Rate-Limit-Remaining: 0.0`.

### 4.6 Concurrency Recommendations for CLI Clients
* **Recommended Concurrency:** **1 to 2 concurrent requests maximum** (or strictly sequential execution).
* **Mathematical Risk:** Because Canvas charges an upfront reservation of **`50.0` points per in-flight request**, launching 12 parallel requests immediately charges $12 \times 50 = 600$ points. This hits the High Water Mark instantaneously and triggers an immediate `403 Rate Limit Exceeded` before any request even finishes processing!
* **Retry Strategy:** Implement exponential backoff with jitter. If `X-Rate-Limit-Remaining < 100.0`, pause outgoing requests until the bucket drains at $10.0$ points per second.

**Citations:**
* [Canvas API Throttling Documentation](https://canvas.instructure.com/doc/api/file.throttling.html)
* [Canvas LMS `RequestThrottle` Source (`app/middleware/request_throttle.rb`)](https://github.com/instructure/canvas-lms/blob/master/app/middleware/request_throttle.rb)

---

## 5. Endpoints Reference

### 5.1 Current User, Dashboard & Activity

| HTTP Method & Path | Key Request Parameters | Key Response Fields | Student CLI Usage | Source Citation |
|---|---|---|---|---|
| `GET /api/v1/users/self` | None | `id`, `name`, `short_name`, `sortable_name`, `login_id`, `email`, `avatar_url`, `time_zone`, `locale`, `bio` | Get authenticated student account identity and user ID. | [Users API](https://canvas.instructure.com/doc/api/users.html#method.users.show) |
| `GET /api/v1/users/self/profile` | None | `id`, `name`, `short_name`, `primary_email`, `login_id`, `avatar_url`, `time_zone`, `bio`, `title`, `pronouns` | Retrieve full profile with contact details and pronouns. | [Users API (Profile)](https://canvas.instructure.com/doc/api/users.html#method.profile.settings) |
| `GET /api/v1/users/self/todo` | `include[]` (`ungraded_quizzes`) | Array of To-Do items: `type` (`submitting`), `assignment` (Assignment object), `quiz`, `course_id`, `html_url`, `ignore`, `ignore_permanently` | Actionable list of assignments and quizzes needing submission. | [Users API (Todo)](https://canvas.instructure.com/doc/api/users.html#method.users.todo_items) |
| `GET /api/v1/users/self/upcoming_events` | None | Array of CalendarEvent/Assignment objects: `id`, `title`, `start_at`, `end_at`, `context_code`, `assignment`, `url`, `html_url` | Upcoming deadlines and calendar events for student dashboard. | [Users API (Upcoming)](https://canvas.instructure.com/doc/api/users.html#method.users.upcoming_events) |
| `GET /api/v1/users/self/missing_submissions` | `include[]` (`planner_overrides`, `course`), `filter[]` (`submittable`) | Array of Assignment objects: `id`, `name`, `due_at`, `points_possible`, `course_id`, `has_submitted_submissions: false`, `planner_override` | Identify overdue, unsubmitted homework across courses. | [Users API (Missing)](https://canvas.instructure.com/doc/api/users.html#method.users.missing_submissions) |
| `GET /api/v1/users/self/activity_stream` | `only_active` (boolean) | Array of stream items: `id`, `title`, `message`, `type` (`DiscussionTopic`, `Announcement`, `Conversation`, `Submission`), `course_id`, `created_at` | Global activity feed across all enrolled courses. | [Users API (Stream)](https://canvas.instructure.com/doc/api/users.html#method.users.activity_stream) |

---

### 5.2 Courses & Favorites

| HTTP Method & Path | Key Request Parameters | Key Response Fields | Student CLI Usage | Source Citation |
|---|---|---|---|---|
| `GET /api/v1/courses` | • `enrollment_state` (`active`, `invited_or_pending`, `completed`)<br>• `enrollment_type` (`student`)<br>• `state[]` (`available`, `completed`)<br>• `include[]`: `term`, `total_scores`, `current_grading_period_scores`, `syllabus_body`, `favorites`, `teachers`, `course_progress`, `concluded` | Array of Course objects:<br>`id`, `name`, `course_code`, `workflow_state`, `term` (`{id, name, start_at, end_at}`), `enrollments` (array containing student role and `grades` object), `syllabus_body`, `is_favorite` | Primary command to list courses with term metadata and current total scores. | [Courses API](https://canvas.instructure.com/doc/api/courses.html#method.courses.index) |
| `GET /api/v1/users/self/favorites/courses` | `exclude_blueprint_courses` (boolean) | Array of favorite Course objects: `id`, `name`, `course_code`, `enrollments` | List student's starred dashboard courses. | [Favorites API](https://canvas.instructure.com/doc/api/favorites.html#method.favorites.list_favorite_courses) |
| `GET /api/v1/courses/:id` | `include[]`: `term`, `syllabus_body`, `total_scores`, `teachers` | Single Course object details. | View course syllabus and metadata. | [Courses API](https://canvas.instructure.com/doc/api/courses.html#method.courses.show) |

---

### 5.3 Assignments & Assignment Groups

| HTTP Method & Path | Key Request Parameters | Key Response Fields | Student CLI Usage | Source Citation |
|---|---|---|---|---|
| `GET /api/v1/courses/:course_id/assignments` | • `bucket`: `past`, `overdue`, `undated`, `ungraded`, `unsubmitted`, `upcoming`, `future`<br>• `include[]`: `submission`, `score_statistics`, `all_dates`, `overrides`<br>• `order_by`: `position`, `name`, `due_at`<br>• `search_term`: text filter | Array of Assignment objects:<br>`id`, `name`, `description` (HTML), `due_at`, `lock_at`, `unlock_at`, `points_possible`, `grading_type`, `submission_types`, `allowed_extensions`, `has_submitted_submissions`, `submission` (current student submission), `score_statistics` (`{min, max, mean}`) | List assignments filtered by deadline bucket, with attached submission status. | [Assignments API](https://canvas.instructure.com/doc/api/assignments.html#method.assignments_api.index) |
| `GET /api/v1/courses/:course_id/assignments/:id` | `include[]`: `submission`, `score_statistics`, `overrides` | Single Assignment object with complete description and requirements. | View assignment instructions, allowed upload formats, and due date. | [Assignments API](https://canvas.instructure.com/doc/api/assignments.html#method.assignments_api.show) |
| `GET /api/v1/courses/:course_id/assignment_groups` | • `include[]`: `assignments`, `submission`<br>• `override_assignment_dates` (boolean) | Array of AssignmentGroup objects:<br>`id`, `name`, `position`, `group_weight` (percentage weight in gradebook), `rules` (`drop_lowest`, `drop_highest`), `assignments` | Inspect syllabus grading breakdown (e.g. Homework 40%, Exams 60%). | [Assignment Groups API](https://canvas.instructure.com/doc/api/assignment_groups.html#method.assignment_groups_api.index) |

---

### 5.4 Submissions & Comments

| HTTP Method & Path | Key Request Parameters | Key Response Fields | Student CLI Usage | Source Citation |
|---|---|---|---|---|
| `GET /api/v1/courses/:course_id/assignments/:assignment_id/submissions/self` | `include[]`: `submission_history`, `submission_comments`, `rubric_assessment` | Submission object:<br>`id`, `attempt`, `body`, `url`, `grade`, `score`, `submitted_at`, `workflow_state`, `late`, `missing`, `excused`, `submission_type`, `attachments`, `submission_comments` (`{id, author_name, comment, created_at, attachments}`) | View current student submission, score, rubric feedback, and instructor comments. | [Submissions API](https://canvas.instructure.com/doc/api/submissions.html#method.submissions_api.show) |
| `GET /api/v1/courses/:course_id/assignments/:assignment_id/submission_summary` | None | `{ "graded": 15, "ungraded": 3, "not_submitted": 2 }` | View high-level class submission distribution. | [Submissions API (Summary)](https://canvas.instructure.com/doc/api/submissions.html#method.submissions_api.submission_summary) |
| `POST /api/v1/courses/:course_id/assignments/:assignment_id/submissions` | • `submission[submission_type]`: `online_text_entry`, `online_url`, `online_upload`, `media_recording`<br>• `submission[body]`: HTML/text body for `online_text_entry`<br>• `submission[url]`: Web URL for `online_url`<br>• `submission[file_ids][]`: Array of uploaded file IDs for `online_upload`<br>• `submission[media_comment_id]` & `submission[media_comment_type]` | Created Submission object (`id`, `attempt`, `submitted_at`, `workflow_state: "submitted"`). | Submit student homework directly from CLI. | [Submissions API (Submit)](https://canvas.instructure.com/doc/api/submissions.html#method.submissions.create) |
| `PUT /api/v1/courses/:course_id/assignments/:assignment_id/submissions/self` | • `comment[text_comment]`: Comment text<br>• `comment[file_ids][]`: Array of attachment IDs | Updated Submission object with newly appended `submission_comments`. | Post a comment to the instructor on a submission. | [Submissions API (Comment)](https://canvas.instructure.com/doc/api/submissions.html#method.submissions_api.update) |

---

### 5.5 Three-Step File Upload Flow

Uploading files to Canvas (for course files, personal files, or assignment submissions) requires a strict 3-step handshake:

```
[Student CLI] --- (1) POST file metadata ---> [Canvas API]
              <-- (2) upload_url + params --
[Student CLI] --- (2) Multipart POST data --> [Storage (AWS S3 / Local)]
              <-- (3) 301/302 Redirect/201 -
[Student CLI] --- (3) Follow redirect / GET -> [Canvas API (Confirm)]
              <-- Finished Attachment JSON -
```

#### Step Details

1. **Step 1: Request Upload Token & Parameters**
   * **Target Endpoints:**
     * Homework submission upload: `POST /api/v1/courses/:course_id/assignments/:assignment_id/submissions/self/files`
     * Submission comment attachment: `POST /api/v1/courses/:course_id/assignments/:assignment_id/submissions/comments/self/files`
     * User personal file upload: `POST /api/v1/users/self/files`
   * **Parameters:** `name` (filename), `size` (bytes), `content_type` (MIME type), `parent_folder_path` (optional).
   * **Canvas Response:**
     ```json
     {
       "upload_url": "https://instructure-uploads.s3.amazonaws.com/",
       "upload_params": {
         "key": "users/1234/files/report.pdf",
         "AWSAccessKeyId": "AKIA...",
         "Policy": "ey...",
         "Signature": "...",
         "success_action_redirect": "https://courses.example.test/api/v1/files/1001/create_success?uuid=..."
       }
     }
     ```

2. **Step 2: Upload File to Storage URL**
   * Send a `multipart/form-data` POST request to `upload_url`.
   * Include all key-value pairs returned in `upload_params` as form fields.
   * **Critical Rule:** The `file` parameter must be the **last** form field.
   * Do **not** send the Canvas `Authorization: Bearer` header to AWS S3.

3. **Step 3: Confirm Upload**
   * S3 responds with `301 Moved Permanently` or `302 Found` containing a `Location` redirect URL pointing back to Canvas.
   * Follow the redirect with a `GET` request (or `POST` with empty body) including `Authorization: Bearer <token>`.
   * Canvas marks the file active and returns the created `File` (Attachment) JSON containing `{ "id": 1001, "display_name": "report.pdf", "size": 12345 }`.
   * The returned `id` can now be passed to `POST .../submissions` in `submission[file_ids][]`.

**Citations:**
* [Canvas API File Uploads Specification](https://canvas.instructure.com/doc/api/file.file_uploads.html)
* [Submissions API (File Upload)](https://canvas.instructure.com/doc/api/submissions.html#method.submissions_api.create_file)

---

### 5.6 Grades & Enrollments

| HTTP Method & Path | Key Request Parameters | Key Response Fields | Student CLI Usage | Source Citation |
|---|---|---|---|---|
| `GET /api/v1/courses?include[]=total_scores` | `include[]=total_scores` | Course list with embedded `enrollments` array containing `grades` object. | Display overall grade across all enrolled courses. | [Courses API](https://canvas.instructure.com/doc/api/courses.html#method.courses.index) |
| `GET /api/v1/users/self/enrollments` | `include[]=grades` | Array of Enrollment objects:<br>`id`, `course_id`, `type: "StudentEnrollment"`, `enrollment_state: "active"`, `grades`: `{ "current_score": 92.5, "current_grade": "A-", "final_score": 81.0, "final_grade": "B-", "html_url": "..." }`, `has_grading_periods`, `current_grading_period_title` | View detailed grades for the student across all semesters. | [Enrollments API](https://canvas.instructure.com/doc/api/enrollments.html#method.enrollments_api.index) |

#### `current_score` vs `final_score`
* **`current_score` / `current_grade`:** Computed using **only assignments that have been submitted and graded**. Unsubmitted assignments are ignored. This matches what students see on their Canvas web dashboard during an active term.
* **`final_score` / `final_grade`:** Computed by treating **all unsubmitted or ungraded assignments as 0 points**. This represents the student's true floor grade if no further work is completed.
* **Grading Periods:** If a course uses grading periods (terms/quarters), enrollments expose `current_period_unposted_current_score`, `current_grading_period_title`, and `current_grading_period_id`.
* *Note:* Gradebook history APIs (`/gradebook_history`) are for instructors/auditors and are **not** accessible to students.

---

### 5.7 Modules & Module Items

| HTTP Method & Path | Key Request Parameters | Key Response Fields | Student CLI Usage | Source Citation |
|---|---|---|---|---|
| `GET /api/v1/courses/:course_id/modules` | `include[]=items`, `include[]=content_details` | Array of Module objects:<br>`id`, `name`, `position`, `state` (`locked`, `unlocked`, `completed`), `prerequisite_module_ids`, `items_count`, `items` | Inspect course syllabus outline, progression prerequisites, and lock status. | [Modules API](https://canvas.instructure.com/doc/api/modules.html#method.context_modules_api.index) |
| `GET /api/v1/courses/:course_id/modules/:module_id/items` | `include[]=content_details` | Array of ModuleItem objects:<br>`id`, `title`, `type` (`Assignment`, `Discussion`, `File`, `Page`, `ExternalUrl`, `ExternalTool`), `content_id`, `html_url`, `completion_requirement` (`{type, min_score, completed}`), `content_details` | List readings, assignments, and files within a specific module. | [Modules API (Items)](https://canvas.instructure.com/doc/api/modules.html#method.context_module_items_api.index) |
| `GET /api/v1/courses/:course_id/module_item_sequence` | `asset_type` (`Assignment`, `DiscussionTopic`, etc.), `asset_id` | Sequence metadata: `items`, `modules`, `current`, `next`, `prev` | Navigate previous/next reading or assignment in syllabus flow. | [Modules API (Sequence)](https://canvas.instructure.com/doc/api/modules.html#method.context_module_items_api.item_sequence) |
| `PUT /api/v1/courses/:course_id/modules/:module_id/items/:id/done` | None | `{ "message": "OK" }` | Mark a "Must Mark Done" module item complete. | [ContextModuleItemsApiController](https://github.com/instructure/canvas-lms/blob/master/app/controllers/context_module_items_api_controller.rb) |
| `DELETE /api/v1/courses/:course_id/modules/:module_id/items/:id/done` | None | Progression state | Unmark a completed module item. | [ContextModuleItemsApiController](https://github.com/instructure/canvas-lms/blob/master/app/controllers/context_module_items_api_controller.rb) |
| `POST /api/v1/courses/:course_id/modules/:module_id/items/:id/mark_read` | None | `{ "message": "OK" }` | Mark a "Must View" item as read. | [Modules API (Mark Read)](https://canvas.instructure.com/doc/api/modules.html#method.context_module_items_api.mark_item_read) |

---

### 5.8 Files & Folders

| HTTP Method & Path | Key Request Parameters | Key Response Fields | Student CLI Usage | Source Citation |
|---|---|---|---|---|
| `GET /api/v1/courses/:course_id/files` | `content_types[]`, `search_term`, `sort`, `order` | Array of File objects:<br>`id`, `folder_id`, `display_name`, `filename`, `content-type`, `url` (download URL), `size`, `created_at`, `updated_at`, `locked_for_user`, `lock_explanation` | Browse and search course downloads (lecture slides, reading PDFs). | [Files API](https://canvas.instructure.com/doc/api/files.html#method.files.api_index) |
| `GET /api/v1/courses/:course_id/folders` | None | Array of Folder objects: `id`, `name`, `full_name`, `folders_url`, `files_url`, `files_count` | Reconstruct folder tree structure in CLI. | [Files API (Folders)](https://canvas.instructure.com/doc/api/files.html#method.folders.list_all_folders) |
| `GET /api/v1/files/:id` | None | Single File object details. | Inspect file metadata and refresh download link. | [Files API (Show)](https://canvas.instructure.com/doc/api/files.html#method.files.api_show) |
| `GET /api/v1/files/:id/public_url` | None | `{ "public_url": "https://...s3.amazonaws.com/...?AWSAccessKeyId=...&Expires=..." }` | Generate presigned direct download link. | [Files API (Public URL)](https://canvas.instructure.com/doc/api/files.html#method.files.public_url) |
| `GET /api/v1/users/self/files` | None | Array of student personal File objects. | View student's personal stored cloud files. | [Files API (User Files)](https://canvas.instructure.com/doc/api/files.html#method.files.api_index) |

#### Download URL Expiration Semantics
* The `url` field on File objects (`https://<canvas>/files/:id/download?download_frd=1`) is an authenticated redirector that issues an `HTTP 302` redirect to a presigned Amazon S3 or Google Cloud Storage URL.
* The redirected storage URL contains expiration signatures (`Expires`, `Signature`) and typically expires within **15 to 60 minutes**.
* Clients must **not** persist or cache raw S3 links. Store the canonical Canvas `file_id` or query `GET /api/v1/files/:id/public_url` to obtain a fresh temporary link on demand.

---

### 5.9 Announcements & Discussions

| HTTP Method & Path | Key Request Parameters | Key Response Fields | Student CLI Usage | Source Citation |
|---|---|---|---|---|
| `GET /api/v1/announcements` | • `context_codes[]` (required, e.g. `course_101`)<br>• `start_date` & `end_date`<br>• `active_only` (boolean)<br>• `latest_only` (boolean) | Array of DiscussionTopic objects where `is_announcement: true`:<br>`id`, `title`, `message` (HTML), `posted_at`, `author` (`{id, display_name, avatar_image_url}`), `url`, `read_state` | Fetch broadcast announcements from professors across enrolled courses. | [Announcements API](https://canvas.instructure.com/doc/api/announcements.html#method.announcements_api.index) |
| `GET /api/v1/courses/:course_id/discussion_topics` | • `order_by`: `position`, `recent_activity`, `title`<br>• `scope`: `locked`, `unlocked`, `pinned`, `unpinned`<br>• `only_announcements` (boolean) | Array of DiscussionTopic objects:<br>`id`, `title`, `message`, `discussion_type` (`side_comment`, `threaded`), `assignment_id`, `require_initial_post`, `user_can_see_posts`, `unread_count` | List course forums and graded discussion assignments. | [Discussion Topics API](https://canvas.instructure.com/doc/api/discussion_topics.html#method.discussion_topics.index) |
| `GET /api/v1/courses/:course_id/discussion_topics/:topic_id/view` | None | Full cached discussion hierarchy: `unread_entries`, `entry_ratings`, `participants`, `view` (nested threaded replies tree) | Render complete conversation thread for reading in terminal. | [Discussion Topics API (View)](https://canvas.instructure.com/doc/api/discussion_topics.html#method.discussion_topics_api.view) |
| `POST /api/v1/courses/:course_id/discussion_topics/:topic_id/entries` | `message` (HTML/text, required), `attachment` (file) | Created DiscussionEntry object: `id`, `user_id`, `message`, `created_at` | Post a top-level reply to a discussion topic. | [Discussion Topics API (Entries)](https://canvas.instructure.com/doc/api/discussion_topics.html#method.discussion_entries_api.create) |
| `POST /api/v1/courses/:course_id/discussion_topics/:topic_id/entries/:entry_id/replies` | `message` (HTML/text, required) | Created Reply DiscussionEntry object. | Reply to another student's post. | [Discussion Topics API (Replies)](https://canvas.instructure.com/doc/api/discussion_topics.html#method.discussion_entries_api.reply_create) |

---

### 5.10 Calendar Events & Planner API

| HTTP Method & Path | Key Request Parameters | Key Response Fields | Student CLI Usage | Source Citation |
|---|---|---|---|---|
| `GET /api/v1/calendar_events` | • `type`: `event` (default) or `assignment`<br>• `context_codes[]`: `course_101`, `user_101`<br>• `start_date` & `end_date` (ISO 8601)<br>• `undated` (boolean) | Array of CalendarEvent objects:<br>`id`, `title`, `start_at`, `end_at`, `description`, `context_code`, `workflow_state`, `assignment` (present if `type=assignment`) | Unified calendar query for dates, office hours, and assignment deadlines. | [Calendar Events API](https://canvas.instructure.com/doc/api/calendar_events.html#method.calendar_events_api.index) |
| `GET /api/v1/planner/items` | • `start_date` & `end_date`<br>• `context_codes[]`<br>• `filter`: `new_activity`, `incomplete_items`, `complete_items` | Array of PlannerItem objects:<br>`context_type`, `course_id`, `plannable_id`, `plannable_type` (`assignment`, `quiz`, `discussion_topic`, `planner_note`), `plannable` (entity object), `submissions` (`{excused, graded, late, missing, needs_grading}`), `planner_override` | Native Canvas Student Planner agenda aggregating all deadlines and tasks. | [Planner API](https://canvas.instructure.com/doc/api/planner.html#method.planner.index) |
| `GET /api/v1/planner_notes` | `start_date`, `end_date`, `context_codes[]` | Array of personal PlannerNote objects: `id`, `title`, `details`, `todo_date`, `course_id`, `workflow_state` | Personal to-do notes created by student. | [Planner Notes API](https://canvas.instructure.com/doc/api/planner.html#method.planner_notes.index) |
| `POST /api/v1/planner_notes` | `title`, `details`, `todo_date`, `course_id` | Created PlannerNote object. | Create a private personal task or reminder. | [Planner Notes API](https://canvas.instructure.com/doc/api/planner.html#method.planner_notes.create) |
| `POST /api/v1/planner/overrides` | • `plannable_type`: `assignment`, `quiz`<br>• `plannable_id`: ID<br>• `marked_complete`: boolean<br>• `dismissed`: boolean | Created PlannerOverride object (`id`, `marked_complete`, `dismissed`). | Allow student to manually check off a completed assignment in planner. | [Planner Overrides API](https://canvas.instructure.com/doc/api/planner.html#method.planner_overrides.create) |

---

### 5.11 Quizzes (Classic vs New Quizzes)

| HTTP Method & Path | Key Request Parameters | Key Response Fields | Student CLI Usage | Source Citation |
|---|---|---|---|---|
| `GET /api/v1/courses/:course_id/quizzes` | `search_term` | Array of Quiz objects:<br>`id`, `title`, `quiz_type` (`practice_quiz`, `assignment`, `graded_survey`), `time_limit` (minutes), `allowed_attempts`, `question_count`, `points_possible`, `due_at`, `lock_at`, `unlock_at`, `locked_for_user`, `lock_explanation` | View Classic Quizzes deadlines, question counts, and time limits. | [Quizzes API](https://canvas.instructure.com/doc/api/quizzes.html#method.quizzes/quizzes_api.index) |
| `GET /api/v1/courses/:course_id/quizzes/:id` | None | Single Quiz object with instructions and settings. | Inspect quiz instructions and status. | [Quizzes API (Show)](https://canvas.instructure.com/doc/api/quizzes.html#method.quizzes/quizzes_api.show) |
| `GET /api/v1/courses/:course_id/quizzes/:quiz_id/submissions` | None | Array of QuizSubmission objects:<br>`id`, `quiz_id`, `user_id`, `submission_id`, `score`, `attempt`, `workflow_state` (`untaken`, `pending_review`, `complete`), `end_at`, `finished_at` | Check whether quiz has been completed and score earned. | [Quiz Submissions API](https://canvas.instructure.com/doc/api/quiz_submissions.html#method.quizzes/quiz_submissions_api.index) |

#### Taking Quizzes via CLI: Policy & Safety
* **DO NOT IMPLEMENT QUIZ-TAKING IN CLI:** Although Canvas REST API technically exposes endpoints to start quiz sessions (`POST .../quizzes/:id/submissions`) and post answers, taking quizzes in a CLI client is strongly discouraged:
  1. Quizzes frequently enforce proctoring features, IP filters, access codes, and lock-down browser integrations (Respondus, Proctorio) that reject plain HTTP clients.
  2. Answer submissions can accidentally trigger academic integrity alerts.
  3. Client network drops or formatting errors can consume limited student attempts with zero recovery options.
* **New Quizzes Notice:** New Quizzes in Canvas are **not** managed by the Classic Quizzes API. New Quizzes run as an external LTI 1.3 Advantage cloud tool. They appear in the Canvas API as Assignments with `submission_types = ["external_tool"]`. The New Quizzes engine operates behind separate LTI OAuth HMAC tokens and cannot be taken or graded through the core Canvas REST API.

---

### 5.12 Conversations (Inbox Messaging)

| HTTP Method & Path | Key Request Parameters | Key Response Fields | Student CLI Usage | Source Citation |
|---|---|---|---|---|
| `GET /api/v1/conversations` | • `scope`: `inbox`, `unread`, `starred`, `sent`, `archived`<br>• `filter[]`: `course_101`, `user_101`<br>• `interleaved`: boolean | Array of Conversation objects:<br>`id`, `subject`, `workflow_state` (`read`, `unread`), `last_message`, `last_message_at`, `message_count`, `audience`, `participants` (`{id, name, avatar_url}`) | Display student's Canvas inbox messages and threads. | [Conversations API](https://canvas.instructure.com/doc/api/conversations.html#method.conversations.index) |
| `GET /api/v1/conversations/:id` | • `auto_mark_as_read` (boolean) | Single Conversation thread with full messages list: `messages` (`{id, author_id, body, created_at, attachments}`) | Read an entire message conversation. | [Conversations API (Show)](https://canvas.instructure.com/doc/api/conversations.html#method.conversations.show) |
| `POST /api/v1/conversations` | • `recipients[]` (user IDs or `course_101_teachers`, required)<br>• `body` (string, required)<br>• `subject` (string, optional)<br>• `group_conversation` (boolean)<br>• `attachment_ids[]` | Created Conversation object. | Send a new message to a teacher or classmate. | [Conversations API (Create)](https://canvas.instructure.com/doc/api/conversations.html#method.conversations.create) |
| `POST /api/v1/conversations/:id/add_message` | • `body` (string, required)<br>• `recipients[]`<br>• `attachment_ids[]` | Updated Conversation object. | Send a reply to an existing message thread. | [Conversations API (Reply)](https://canvas.instructure.com/doc/api/conversations.html#method.conversations.add_message) |

---

### 5.13 External Tools & LTI: CLI Boundaries
Modern Canvas courses heavily integrate external tools (LTI 1.1 / LTI 1.3 Advantage) such as Gradescope, McGraw-Hill Connect, Pearson MyLab, WileyPLUS, Top Hat, Cengage, and New Quizzes.

#### What a CLI Client CANNOT Do
* **Cannot Execute LTI Launches:** LTI tools require a browser-based OpenID Connect (OIDC) handshake and signed HTML form `POST` requests (`id_token`, state) rendered inside iframes or browser windows.
* **Cannot Bypass Third-Party Auth & Cookies:** LTI vendor platforms maintain their own session state, third-party cookies, and proprietary JavaScript frontends.
* **Cannot Submit Third-Party Homework:** External tools report scores back to Canvas asynchronously using LTI Assignment and Grade Services (AGS). There is no Canvas REST API endpoint for a student to submit an external LTI assignment directly.

#### What a CLI Client SHOULD Do
* When an assignment has `submission_types = ["external_tool"]`:
  1. Detect the external tool configuration (`external_tool_tag_attributes`).
  2. Display the external tool name and launch URL.
  3. Provide a command or helper to open the assignment launch URL directly in the student's default web browser:
     ```bash
     canvas open <assignment_id>
     ```

**Citations:**
* [Canvas API External Tools](https://canvas.instructure.com/doc/api/external_tools.html)
* [LTI Launch Overview](https://canvas.instructure.com/doc/api/file.lti_launch_overview.html)

---

## 6. GraphQL API (`/api/graphql`)

### 6.1 Overview & GraphiQL Explorer
Canvas includes a production GraphQL endpoint alongside the REST API:
* **Endpoint:** `POST https://<school-hostname>/api/graphql`
* **Interactive Schema Explorer:** `https://<school-hostname>/graphiql` (accessible in-browser when logged in)
* **Authentication:** Uses the exact same `Authorization: Bearer <token>` header as the REST API.
* **Content-Type:** `application/json` (or form POST with `query` and `variables`).

### 6.2 The Power of GraphQL for CLI Clients: Batching
The Canvas REST API suffers from extensive $N+1$ query overhead: to load a dashboard, a client must make separate HTTP requests for user profile, courses, assignments per course, submissions per assignment, unread announcements, and todo items. Under the leaky bucket rate limiter, this rapidly consumes quota.

With GraphQL, a CLI client can fetch the entire dashboard tree in a **single round-trip HTTP request**:
```graphql
query StudentDashboard {
  allCourses {
    _id
    name
    courseCode
    term {
      name
    }
    assignmentsConnection(first: 20) {
      nodes {
        _id
        name
        dueAt
        pointsPossible
        submissionTypes
        submissionsConnection {
          nodes {
            score
            grade
            submittedAt
            workflowState
          }
        }
      }
    }
  }
}
```

### 6.3 IDs in GraphQL: Relay `id` vs Database `_id`
Canvas GraphQL adheres to the Relay specification:
* `id` returns an opaque base64 global identifier (e.g. `"Q291cnNlLTE="`).
* `_id` returns the traditional integer database ID used in the REST API (e.g. `"1"`).
* The query root provides:
  * `node(id: ID!)`: Fetch an object by global Relay ID.
  * `legacyNode(type: LegacyNodeType!, _id: ID!)`: Fetch an object by REST integer ID.
  * Helper fields such as `course(id: "1")` which accept either GraphQL or REST IDs.

### 6.4 Pagination in GraphQL
Uses standard Relay Connection specification:
```graphql
{
  course(id: "1") {
    assignmentsConnection(first: 10, after: "YXJyYXljb25uZWN0aW9uOjEw") {
      nodes {
        _id
        name
      }
      pageInfo {
        endCursor
        hasNextPage
        totalCount
      }
    }
  }
}
```

### 6.5 Known Limits of Canvas GraphQL
1. **Read-Focused Schema:** Very few mutations exist in the GraphQL schema. Actions like uploading files, submitting homework, or sending conversations must still use REST.
2. **Schema Incompleteness:** Not all REST resources or newer feature flags are exposed in GraphQL.
3. **AST Query Complexity Limits:** Instructure enforces query depth and node-count limits on GraphQL requests to prevent database exhaustion attacks. Overly nested queries will receive a GraphQL validation error.

**Citations:**
* [Canvas GraphQL API Reference](https://canvas.instructure.com/doc/api/file.graphql.html)

---

## 7. Time and Date Handling

### 7.1 ISO 8601 UTC Standard
* **All Timestamps are UTC:** The Canvas API normalizes all date and timestamp fields to UTC using the standard ISO 8601 format:
  ```
  YYYY-MM-DDTHH:MM:SSZ
  Example: 2026-09-15T23:59:00Z
  ```
* Even if an institution, course, or user has set their local timezone to Eastern Time (`America/New_York`), API JSON responses **always** terminate in `Z` (Zulu/UTC).

### 7.2 Timezone Fields & CLI Conversion
To present dates accurately to students, the CLI must parse the UTC timestamp and format it in the appropriate timezone:
* **User Preferred Timezone:** Available via `GET /api/v1/users/self`:
  * `time_zone`: IANA timezone string, e.g. `"America/New_York"` or `"America/Denver"`.
* **Course Timezone:** Available via `GET /api/v1/courses/:id`:
  * `time_zone`: Course-specific timezone string (useful for distance-learning courses operating in a different zone from the student).

### 7.3 `due_at`, `lock_at`, and `unlock_at` Null Rules
Assignment availability is governed by three primary timestamp fields:

| Field | Meaning when Populated | Meaning when `null` |
|---|---|---|
| `due_at` | The deadline when submissions are due. Submissions after this date are flagged as `late: true`. | **Undated Assignment:** The assignment has no deadline. It never becomes overdue based on clock time. |
| `unlock_at` | Submissions and instructions are hidden/locked until this timestamp. | **Immediately Available:** Submissions are accepted as soon as the assignment is published. |
| `lock_at` | Hard closing cutoff. Submissions are strictly prohibited after this timestamp. | **No Hard Cutoff:** Late submissions are accepted indefinitely past `due_at` until the instructor manually locks it or concludes the course. |

#### Overrides and `all_dates`
* Assignments can have differentiated due dates for specific sections or students.
* When requesting assignments, Canvas defaults to `override_assignment_dates=true`. This ensures `due_at`, `lock_at`, and `unlock_at` reflect the **effective date for the calling student**.
* Passing `include[]=all_dates` returns an array of all date overrides for the assignment.

**Citations:**
* [Canvas API Assignments Reference](https://canvas.instructure.com/doc/api/assignments.html)

---

## 8. Error Responses & Status Codes

### 8.1 Error Response Shapes
Canvas returns errors in several JSON formats depending on the layer that encountered the failure:

#### Format A: Standard Controller Validation / Business Logic Errors
```json
{
  "errors": [
    {
      "message": "The assignment is locked."
    }
  ]
}
```

#### Format B: Simple Array of Strings (Batch / Async APIs)
```json
{
  "errors": [
    "start_date and end_date must be the first day of the month",
    "end_date must be after start_date"
  ]
}
```

#### Format C: Top-Level Message Object
```json
{
  "message": "Invalid access token."
}
```

#### Format D: OAuth2 RFC Specification Error
```json
{
  "error": "invalid_grant",
  "error_description": "code not found or expired"
}
```

#### Format E: Throttling Plaintext
```text
403 Forbidden (Rate Limit Exceeded)
```

### 8.2 Common HTTP Status Codes

| HTTP Status | Typical Cause in Canvas | Recommended CLI Handling |
|---|---|---|
| **401 Unauthorized** | Missing, malformed, or expired token. Or token used against wrong institution domain. `WWW-Authenticate` header present. | Prompt user to verify API token or trigger OAuth2 refresh flow. |
| **403 Forbidden** | • Insufficient role permissions.<br>• Assignment/Module locked.<br>• Rate limit exceeded (`Rate Limit Exceeded`). | Check response body. If rate limited, trigger backoff. Otherwise notify student that item is locked or access is denied. |
| **404 Not Found** | Resource does not exist, **or** student lacks permission to know it exists (Canvas uses 404 to avoid leaking private IDs). | Inform user that course, file, or assignment was not found. |
| **422 Unprocessable Entity** | Parameter validation error (e.g. invalid date range, illegal file type). | Validate CLI input arguments against allowed schema. |
| **429 Too Many Requests** | Rate limit exceeded (when enabled by admin). | Execute exponential backoff. |

### 8.3 Masquerading: N/A for Students
Canvas includes an administrative masquerading feature allowing administrators to view the API as another user by appending `?as_user_id=:user_id`.
* This capability is restricted strictly to users with the `Become other users` account permission.
* For student accounts, attempting to pass `as_user_id` returns `HTTP 401 Unauthorized` or `HTTP 403 Forbidden`. It has no applicability in a student CLI.

**Citations:**
* [Canvas API Masquerading](https://canvas.instructure.com/doc/api/file.masquerading.html)
* [Canvas API Endpoint Attributes](https://canvas.instructure.com/doc/api/file.endpoint_attributes.html)

---

## 9. Student Permissions & CLI Edge Cases

A student-facing CLI client operates in a highly constrained permission environment compared to administrative tools. The following constraints and feature flags must be anticipated:

1. **Disabled Personal Access Tokens:**
   * Institutions can disable personal token generation for students. If enabled, the CLI cannot use a simple manual copy-paste token and must implement an OAuth2 flow or instruct the user to request assistance.
2. **Hidden Navigation Tabs & Locked Endpoints:**
   * Instructors frequently hide navigation items in Course Settings (e.g. hiding the "Files", "Pages", or "Modules" tabs).
   * In Canvas, if the instructor disables the "Files" tab, querying `GET /api/v1/courses/:id/files` returns **`HTTP 403 Forbidden`** for students, even if files exist in the course. Files can only be downloaded if linked inside an accessible module item or assignment description.
3. **Concluded or Future Terms:**
   * Institution settings can restrict students from viewing course materials before the term begins or after the term concludes (`restrict_student_past_view`, `restrict_student_future_view`). Attempting to access these courses returns `403 Forbidden`.
4. **Hidden Grades & Score Statistics:**
   * Instructors can enable **"Hide totals in student grades summary"** or **"Hide grade distribution scoring"** in course settings.
   * When enabled, `total_scores` will be `null` in the course enrollments object, and `score_statistics` (`min`, `max`, `mean`) will be omitted from assignment payloads.
5. **Sequential Progression & Prerequisite Locks:**
   * Modules can enforce sequential progression (students must complete Item A before Item B unlocks) or require a minimum score on a prerequisite quiz. Module items will report `locked_for_user: true` and include a `lock_explanation`.
6. **LTI & New Quizzes Inaccessibility:**
   * Assignments powered by external LTI tools or New Quizzes cannot be taken or submitted via the Canvas REST API. The CLI must detect `submission_types: ["external_tool"]` and redirect the student to their web browser.
7. **Proctored Exams:**
   * Quizzes requiring Respondus LockDown Browser, Proctorio, or classroom IP filters cannot be accessed by non-browser HTTP clients.

---

## 10. Summary Verification Matrix

| Area | Verified / Unverified | Verification Notes |
|---|---|---|
| Manual Token Generation & Admin Restrictions | **VERIFIED** | Verified via Canvas User Settings UI, Admin Guide, and API Policy docs. |
| OAuth2 3-Step Flow & Expiry | **VERIFIED** | Verified via `file.oauth.html` and `file.oauth_endpoints.html`. Access tokens expire in 1 hr; refresh tokens persist. |
| Base URL & Lasell University Deployment | **VERIFIED** | Live probed `courses.example.test` and `courses.lasell.edu`; verified Microsoft Entra ID SAML SSO and AWS `cluster90`. |
| Pagination & Link Headers | **VERIFIED** | Verified via `file.pagination.html` and Canvas source code (`lib/api.rb`); `MAX_PER_PAGE = 100`. |
| Leaky Bucket Throttling Defaults | **VERIFIED** | Verified directly in Canvas LMS source code (`app/middleware/request_throttle.rb`); HWM 600/700, Outflow 10/s, Upfront 50. |
| REST Endpoints & Parameters | **VERIFIED** | Verified against official API documentation for Users, Courses, Assignments, Submissions, Modules, Files, Announcements, Calendar, Planner, Quizzes, Conversations. |
| 3-Step File Upload Protocol | **VERIFIED** | Verified via `file.file_uploads.html` (metadata request -> multipart POST to S3 -> confirm redirect). |
| GraphQL Capabilities & Batching | **VERIFIED** | Verified via `file.graphql.html` and GraphiQL explorer documentation. |
| UTC Timestamps & Timezone Semantics | **VERIFIED** | Verified via ISO 8601 UTC rules and course/user timezone fields in API specs. |
| Error Formats & Masquerading | **VERIFIED** | Verified via API error response schemas; student masquerading confirmed impossible (admin-only). |
