# M0-b — API contracts in `crates/canvas-api` (Cursor Auto, lane w1)

Read `docs/SPEC.md` §11 (client), §7 (ID/timestamp rules), §10 dataset table
(which endpoints and includes exist), Appendix B (endpoints), §16 row 1
(tests), Appendix A (versions). Implement the API crate contracts. No disk,
no config, no output rendering: this crate knows only HTTP and models.

## Deliverables
1. `Client::new(origin: Url, token: Secret, user_agent: &str)` with `reqwest`
   (rustls, gzip, brotli, `redirect(Policy::none())`, connect 10 s, request 30 s).
2. **Request phases** (§11): `ApiRequest` and `TransferRequest` builders. API
   requests: bearer only on same-origin; redirects same-origin only, ≤5 hops,
   303 → GET, 301/302 → GET only if the original was GET else
   `Error::UnexpectedRedirect`, 307/308 → same method and body; `Link`
   `rel="next"` must be same-origin else `Error::CrossOrigin`. Transfer
   requests: initial URL https only; token only same-origin; ≤5 hops; download
   hops as GET; any redirect of an upload POST → `Error::UploadIncomplete`.
   (Upload and download bodies themselves are M2-a; only the phase rules and
   a `send_transfer` primitive land here.)
3. `get<T>`, `get_all<T> -> impl Stream<Item = Result<Page<T>>>` with
   `per_page=100` and Link parsing, `post`, `put`, `delete`; a
   `WrappedCollection<T>` adapter for `{ "grading_periods": [...], "meta": ... }`.
   Server `4xx` with an `errors` body → `Error::Validation { status, errors }`.
4. **Governor** (§11): API and storage semaphores (`api_concurrency` default 4
   max 8), issue sequence numbers, observations applied only when newer by
   issue number, refill estimate `min(10/s, observed)`, cooldown below 150
   with 1 in flight and wait toward 300, cooldown ends at an applied sample
   ≥ 300, header-silence reset after 60 s only when nothing is in flight,
   retries on 429 / 403 "Rate Limit Exceeded": initial + 4, delays 1/2/4/8 s
   ±25 % jitter or `Retry-After`, retries pass the admission gate.
   Sum `X-Request-Cost` into a `Telemetry { api, storage, cost }` counter.
5. **Redaction**: a `tracing` layer plus the error `Display` path redact
   `Authorization`, `access_token`, every `upload_params` value, and query
   parameters `Signature`, `X-Amz-*`, `Policy`, `Expires`, `verifier`, `sig`,
   `token`.
6. **Models** for every v1 entity: users/self, courses (with `term`,
   `enrollments[].computed_*`, `current_period_computed_*`), terms,
   enrollments (`grades`), grading periods, assignment groups (rules),
   assignments (incl. `submission`, `can_submit`, `allowed_attempts`,
   `group_category_id`, `external_tool_tag_attributes`), submissions
   (`submission_history`, `attachments`, `submission_comments`,
   `rubric_assessment`), planner items (`plannable_type`, `plannable`,
   `plannable_date`, `planner_override`), missing submissions, folders, files,
   modules (`items`, `items_count`, `state`), module items (`content_details`),
   announcements (discussion topics), calendar events (`all_day`,
   `all_day_date`). Rules: `#[serde(default)]`, `i64` IDs from numbers or
   strings, `jiff::Timestamp` with offsets normalized, `jiff::civil::Date` for
   civil dates, relative URLs resolved against the origin, absent and `null`
   collapsed to `None`.
7. `Error` enum per §11 (`Unauthorized`, `Forbidden { rate_limited, body }`,
   `Denied { status }`, `NotFound`, `RateLimited`, `Validation`, `CrossOrigin`,
   `UnexpectedRedirect`, `UploadIncomplete`, `StorageExpired`, `SizeMismatch`,
   `Network`, `Timeout`, `Decode`).
8. Hand-written fixtures under `crates/canvas-api/tests/fixtures/` (one JSON
   per model, plus a 3-page paginated set and a wrapped grading-periods set
   across 2 pages) and `wiremock` tests for every §16 row-1 item except
   upload and download bodies: pagination over 3 pages; cross-origin `next`
   rejected; same-origin 303/307 followed, off-origin rejected, 301 on POST
   rejected; governor: delayed high sample discarded, header silence with and
   without in-flight, cooldown recovery under a continuous cost-1 workload,
   `Retry-After`, initial + 4 retries with exact delays (use tokio paused
   time); redaction of every listed key; absent/null/value model fixture.

## Rules
- Owner files: `crates/canvas-api/**` only. Do not touch other crates, `docs/`,
  or `tasks/`. Do not add workspace dependencies without noting it.
- Work on the current branch `lane/w1` (already checked out). Commit as you go
  with conventional messages. Do not push. Do not merge into `main`.
- Your private build dir is `CARGO_TARGET_DIR` (already exported in this shell).
- Use the crate versions from SPEC Appendix A.

## Gates (all must pass before you report)
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo +1.88 check --workspace --all-targets
```

Finish with `git status --short` and reply exactly: `DONE M0-b`
