MERGE
M1-b defects are fixed in six review commits on lane/w2; no spec change is required.
All five gates pass; 194 tests pass, including 18 added regressions.

## Scope

Reviewed `tasks/m1b-resolve-courses.md`, its cited SPEC sections and public API/store contracts, the worker history (`2ff1aa0`, `c817b28`), and the complete change against main. The required initial `git merge main` completed cleanly as `06488a6` (main at `137256a`). No later merge or push was performed.

This verdict covers M1-b, including its temporary session bridge. M0-c still owns full authentication, credential-store/env-binding selection and config integration; those features are not implemented by this package and this review does not certify their integration or a complete v1 release. `sync --full` assembly remains M4-b's assigned work. Network evidence below comes from local mock servers, not a live Canvas account.

## Gates

All Cargo gates used `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/rev-m1b`.

| Gate | Baseline | Final |
|---|---|---|
| `cargo fmt --all --check` | PASS | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS | PASS |
| `cargo nextest run --all-features` | PASS, 176 tests | PASS, 194 tests, zero skipped |
| `cargo deny check` | PASS | PASS: advisories, bans, licenses and sources; existing duplicate-version warnings only |
| `cargo +1.88 check --workspace --all-targets` | PASS | PASS |

Final nextest output used `--status-level fail --final-status-level fail` to reduce successful-test logging; the test selection was unchanged. Dependency declarations added during review reuse the exact Appendix A workspace pins (`reqwest`, `sha2`, `wiremock`); no dependency version was changed. `git diff --check` also passed.

## Defects found and fixed

Locations refer to the final implementation unless explicitly described as an original behavior. Severity: High = incorrect data, isolation or essential command behavior; Medium = contract, diagnostics, persistence protocol or verification gap.

| Severity | File:line | What was wrong | What changed | Commit |
|---|---|---|---|---|
| High | `crates/canvas-core/src/resolve.rs:413` | A cached assignment could override the supplied course, including after an otherwise matching URL. | Reject cached course/assignment disagreement with the resolution error; test numeric and URL forms. | `55c98f4` |
| Medium | `crates/canvas-core/src/resolve.rs:444` | Hand-split origins rejected equivalent canonical URLs; assignment path pieces could be unrelated. | Parse URLs with the existing pinned URL implementation, compare canonical origins, reject userinfo, and require adjacent course/assignment path segments. | `55c98f4` |
| Medium | `crates/canvas-core/src/resolve.rs:191` | Alias writes used implicit SQLite transactions instead of the required `BEGIN IMMEDIATE`. | Explicit immediate transactions for set/remove, preserving identity-local alias storage. | `55c98f4` |
| Medium | `crates/canvas-core/src/resolve.rs:278`; `crates/canvas-cli/src/commands/emit.rs:125` | Zero-match candidates were discarded; CLI ambiguity errors hid available candidates. | Retain membership candidates and render them on stderr or as string-ID records in error details. | `55c98f4`, `d3d266f` |
| High | `crates/canvas-core/src/sync/wire.rs:28`; `crates/canvas-core/src/sync/courses.rs:275` | Optional fields lost explicit-null observations; course extras, period metadata and grading-period extras bypassed field clocks. Older responses could overwrite newer values. | Preserve absent/null/value during ingestion, allowlist fields, and apply observation clocks to every stored field, including extras. Embedded fields remain independently clocked. | `3050102` |
| High | `crates/canvas-core/src/sync/courses.rs:84`; `crates/canvas-core/src/sync/courses.rs:652` | Embedded terms/totals were only entity upserts, without membership or coverage. Course refreshes also lacked a guard against concurrent totals-epoch changes. | Publish derived coverage inside the course transaction and include totals epochs in pre-fetch/commit/cache-hit checks. Add backward-compatible Dataset hooks. | `3050102` |
| High | `crates/canvas-core/src/sync/course_totals.rs:93`; `crates/canvas-cli/src/commands/course_load.rs:254` | Whole-course totals acquired current-period metadata; CLI always selected whole-course scores. The normal Canvas `student` summary type was not recognized explicitly. | Recognize student summaries, keep whole-course period metadata null, and select current totals for courses with grading periods. | `3050102`, `d3d266f` |
| High | `crates/canvas-cli/src/commands/course_load.rs:475` | A period change with omitted scores could display the old period's scores under the new title. | Record the period transition without clearing absent source fields, then expose only scores observed for the applicable period; otherwise return null/unavailable. | `bc03550` |
| High | `crates/canvas-cli/src/commands/course_load.rs:29` | Courses' six-hour TTL also governed grade reads; independent grade age/coverage was not reported. | Honor the shorter grades TTL, refresh its supplying endpoint when needed, and append totals freshness using coverage and field ages. | `d3d266f` |
| Medium | `crates/canvas-core/src/sync/refresh.rs:119`; `crates/canvas-cli/src/commands/course_load.rs:433` | A fresh cache hit was returned before the offline branch, producing `stale:false` offline. | Offline reads always report stale; complete empty memberships still succeed. Test the actual refresh path, not a copied offline helper. | `3050102`, `d3d266f` |
| Medium | `crates/canvas-core/src/sync/refresh.rs:119` | Refresh counts omitted failed/retried requests, and duplicated courses inflated network freshness counts. Failure-marker persistence errors were swallowed. | Use client telemetry deltas, count unique entities, and propagate persistence failures. Failed later pages retain the prior complete data. | `3050102` |
| High | `crates/canvas-cli/src/commands/course.rs:21`; `crates/canvas-core/src/sync/course.rs:63` | `course` never fetched Appendix B's detail endpoint, fetched an unrelated active list, suppressed failures and fabricated an empty success without coverage. | Fetch/cache one detail object with separate coverage; preserve active/all memberships; return offline miss without detail coverage. Project teachers to id/name and convert syllabus HTML to Markdown. | `3050102`, `d3d266f` |
| High | `crates/canvas-cli/src/commands/course.rs:102` | Name resolution permitted only one refresh, so cold resolution needing both active and all memberships failed prematurely. | Refresh each required scope once, retain resolution freshness, and continue to detail retrieval. Regression verifies the five-request cold completed-course path. | `d3d266f` |
| High | `crates/canvas-cli/src/commands/sync.rs:91` | `sync` reused fresh cache by default, omitted derived datasets, and could claim success despite stale refresh errors or stop at the first per-course denial. | Always refresh the assigned datasets, report derived coverage, continue across per-course denials, sort results, and return partial/12 with scope/status evidence. | `d3d266f` |
| High | `crates/canvas-cli/src/session.rs:167`; `crates/canvas-cli/src/commands/emit.rs:104` | A new environment token could populate another selected identity's cache without validation. API failures were flattened to exit 4 and lost HTTP status/request cost. | Validate newly seen token hashes before ingestion, reject identity mismatch, retain only the hash, and use typed error classifications plus actual invocation telemetry. Cache/local operations do not depend on token validity. | `d3d266f` |
| Medium | `crates/canvas-cli/src/session.rs:72`; `crates/canvas-cli/src/commands/emit.rs:37` | The temporary bridge silently ignored corrupt config, allowed an environment key to beat explicit profile selection, mapped identity failures to auth, and exposed a profile on errors before selection. | Validate config/profile/key consistency, reject unsafe key components before file reads, distinguish local errors, and emit null pre-selection identity/profile. Refuse a mismatched env origin before any token use. | `d3d266f` |
| Medium | `crates/canvas-core/src/sync/wire.rs:49` | Additional detail metadata could carry arbitrary non-scalar data if projected without typed validation. | Reject malformed count/boolean metadata before ingestion; retain only allowlisted detail fields. | `16ad26a` |
| Medium | `crates/canvas-cli/src/main.rs:551`; `crates/canvas-cli/src/output/envelope.rs:139` | `version --json` was plain text; unimplemented commands emitted no JSON document; serializer failure could start a partial document. | Use the version/error envelopes and serialize successfully before writing stdout. Keep clap's documented raw-command/usage behavior. | `d7cc65b` |
| Medium | `crates/canvas-cli/src/output/registry.rs:332`; `crates/canvas-cli/src/output/render.rs:26` | Registry tests checked parseability without snapshotting all rendered schemas, and table width ignored the frozen `COLUMNS` setting. Required runtime cases were covered only by simulations. | Snapshot every registered envelope with unique schema IDs and meaningful metadata, honor `COLUMNS`, fix the offline snapshot and freeze TZ, and add real refresh/CLI regressions. | `3050102`, `d3d266f`, `d7cc65b` |

## Verification evidence

The added tests exercise canonical URLs and assignment-course binding; actual fresh/cache/offline reads including complete count zero; exactly three `--all` state requests and duplicate membership; a successful first page followed by a denied second page; two wrapped grading-period pages; null/absent/out-of-order observations; concurrent totals epoch invalidation; detail caching and allowlisted Markdown/teacher records; token identity mismatch; API abort status/counts; sync refresh and partial reporting; grade TTL expiry and period-label isolation; profile/config failures; and identity-free version JSON.

Existing ambiguity/incompleteness, scoped enrollment P/Q, field-observation ordering, cache-clear/epoch, newer-schema, concurrent-store and containment tests remain green. Raw-output flag rejection remains tested at every command level. No raw response body, teacher email or fixture token is persisted by the exercised detail/failure paths. The report makes no live keyring, Windows-runtime or real-account claim.

## Needs a decision

None for the M1-b specification. The existing M0-c authentication/config integration and M4-b full-sync ownership remain as described in the package brief; they are separate integration work, not a relaxation of their SPEC requirements.
