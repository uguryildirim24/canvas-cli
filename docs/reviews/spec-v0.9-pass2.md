# Review :  SPEC v0.9 consolidation pass 2 (lane `w3`)

Verdict: **MERGE**, after the eleven `review(SPEC-v0.9):` commits below.

Reviewed 2026-09-10 in `<checkout>` on
branch `lane/w3`, after `git merge main` brought in
`tasks/review-spec-v09-pass2.md` (a clean docs-only merge, `c63641f`).
`git diff main HEAD -- crates xtask skill extension Cargo.toml Cargo.lock` is
empty, so every "as built" claim was checked against the tree in this
worktree, which is `main`'s tree.

Brief: `tasks/spec-v09-pass2.md`. Reviewer brief:
`tasks/review-spec-v09-pass2.md`.

The rule under review was: every normative sentence added to `docs/SPEC.md` is
true of the code on `main`, no new decision is made, and no §19 item is
resolved except by a one-line "resolved by …" note naming a real commit.
Sentences were checked against modules, manifests, migrations, registry
entries, fixtures and tests :  not against the REPORT, the briefs, or the
worker's report. Ten sentences failed and one gap in the code was silently
passed over; all eleven are handled below. Nothing else in §24, §25, the
extended tables, or the appendices contradicted the code.

Build and gate target: `CARGO_TARGET_DIR=<checkout>`.

## What was corrected

| Section | What was wrong | What it says now | Evidence |
|---|---|---|---|
| §21.2 tool catalog | "`context.note` and `context.follow` are the two `Organize` tools that are not idempotent." Seven of the ten are: `download.run`, all four `*.prepare` tools, `context.note` and `context.follow` carry `idempotent: false`. | Names the seven, and names `sync.run`, `context.attach` and `context.detach` as the three that repeat safely. | `crates/canvas-cli/src/mcp/catalog.rs` (`ToolSpec` rows; `sync.run` :866, `download.run`, `submission.prepare` :902) |
| §24.1 lifecycle | "Sharing pauses on a hidden tab …, on entering an assessment, on a tab close, and on host loss." A tab close and a cross-origin navigation **end** the attachment (`end()` sends `detach`), and host loss sends nothing at all :  the port is gone. | Splits pause from end, and says host loss is a disconnected port reported to nobody. | `extension/src/background.js:72-94`, `:96-124`, `:218-222`, `:386-401` |
| §24.6 `bridge-native@1` | "A pause carries its cause: `hidden`, `assessment`, `tab_closed`, `user_detached`, or `cross_origin`." The five are the shared `PauseCause` enum; only `hidden` and `assessment` ever arrive on a `pause`. | One cause list, and the message says which half it is: `pause` for the first two, `detach` for the other three. | `crates/canvas-core/src/bridge/wire.rs:212-223`; `extension/src/content.js:142`; `extension/src/background.js:178-181`, `:386-401` |
| §24.16 what has not been run | The not-run list dropped "a stale follow refused with a real tab behind it", which `docs/companion.md` carried and which this pass reduced to a pointer. The check table still has the row. | The item is back on the list. The refusal itself is tested; only the real tab is not. | `docs/companion.md` on `main`, "Not run"; `crates/canvas-cli/tests/m7b.rs:307` |
| §25.3 and §14, `unresolved` | "a `--to` entry outside the topic, or a recipient id Canvas does not return", presented as the exact conditions. A third path raises it: an attachment that cannot be canonicalized, is not a regular file, or whose name or path is not valid UTF-8. | Both tables name the attachment case. | `crates/canvas-core/src/operations/prepare.rs:419-455` |
| §25.5 recovery | "Only `execute` and `operation reconcile` recover." `prepare` recovers too, through the same `ops::recover_active` call. | "`prepare`, `execute`, and `operation reconcile` recover; `operation status` does not." | `crates/canvas-core/src/operations/prepare.rs:296`; `execute.rs:119`; `reconcile.rs:82-98`, `:148-156` |
| §25.6 response record | "A readback stores the same fields plus `scanned` and `complete`." `OperationReadback` drops `conversation_id` and `response_sha256` and adds `read_at`. Appendix D already had this right. | Names the six fields it keeps, the three it adds, and the two it does not hold. | `crates/canvas-core/src/operations/record.rs:410-464` |
| Appendix A, `rustix` | The note was extended to "for the socket and pipe permission work of §24". `rustix` entered at M0-c (`3ded806`) and its three uses are the credential file's `O_NOFOLLOW` open and `geteuid`. The broker's modes use `std::os::unix::fs::PermissionsExt`. | Names the credential file's no-follow open and owner check. | `crates/canvas-cli/src/credentials.rs:743`, `:771`, `:816`; `crates/canvas-cli/src/bridge/owner.rs:147-151` |
| §19 item 44 | "`docs/companion.md` recorded both deviations." The same pass reduced that file to a pointer; it records neither. | "§24.1 records both deviations." | `docs/companion.md`; SPEC §24.1 permission table |
| `docs/writes-v2.md` | "the schemas, the six tools, and the sixth skill workflow". §25.10 announces eight. Six is the count of *write* tools the skill test confines to one workflow, a different set. | "the eight tools". | SPEC §25.10; `crates/canvas-cli/tests/skill.rs:67-78` |
| §24.10 and §19 item 46 | §24.10 said journal states are drawn with their exact §12.2 names and only `submitted` is finished :  true, but silent about operation journals, which the panel also lists and for which it has no words. | §24.10 says what the panel's sentences cover; **new §19 item 46** names the gap. No code was touched. | `crates/canvas-cli/src/bridge/panel.rs:245-267`; `crates/canvas-core/src/receipts/ops.rs:310-320`; `extension/src/panel_view.js:20-42` |

Commits, oldest first:

| Commit | Message |
|---|---|
| `0123c49` | `review(SPEC-v0.9): correct which Organize tools are idempotent` |
| `2f2232e` | `review(SPEC-v0.9): separate a pause from a detach in §24.1 and §24.6` |
| `58856c9` | `review(SPEC-v0.9): put the stale follow back on §24.16's not-run list` |
| `12fe122` | review(SPEC-v0.9): name the attachment case under `unresolved` |
| `5716f6a` | `review(SPEC-v0.9): name prepare as a recoverer in §25.5` |
| `9b54d00` | `review(SPEC-v0.9): correct the readback's fields in §25.6` |
| `3d6c9d4` | `review(SPEC-v0.9): say what rustix is actually for in Appendix A` |
| `1b9e599` | `review(SPEC-v0.9): point item 44 at §24.1, not at the pointer file` |
| `0c9eceb` | `review(SPEC-v0.9): the writes pointer names eight tools, not six` |
| `81f4e9f` | `review(SPEC-v0.9): record the panel's missing operation-state words` |
| `e0c4451` | `review(SPEC-v0.9): record item 46 in the changes file` |

No code and no test was touched. Nothing was pushed and nothing was merged
into `main`. Nothing was installed into any Chrome profile, and no identity or
credential was read or written.

## What was checked and found true

### §24.1 The extension

`extension/manifest.json` is Manifest V3, `minimum_chrome_version` `"116"`,
and its `permissions` array is exactly `["activeTab", "nativeMessaging",
"scripting", "sidePanel"]`. It has no `host_permissions`, no
`content_scripts`, no `externally_connectable`. `tests/companion.rs`
`the_manifest_asks_for_nothing_it_does_not_need` asserts the four names as an
ordered equality and refuses each of the eleven the SPEC lists (plus
`webRequestBlocking` and `<all_urls>`, which the SPEC does not name :  the SPEC
list is a subset of the test's, which is the safe direction).
`the_panel_builds_no_markup_from_a_string` scans every `.js` the panel page
loads for all seven strings §24.1 names, and asserts it scanned at least five
files. `extension/package.json` declares no `dependencies`,
`devDependencies` or `peerDependencies`, and its test script is
`node --test`, so `npm test` installs nothing.

Injection is `chrome.scripting.executeScript` from the `action.onClicked` and
`commands.onCommand` ("attach", `Alt+Shift+C`) handlers only. A second gesture
on the attached tab calls `end("user_detached")`. `tabs.onUpdated` with
`status === "complete"` increments the generation on the same origin and calls
`end("cross_origin")` otherwise. `stamp()` puts `tab_id` and
`navigation_generation` on every observation. The worker performs no `fetch`
and reads no page; the `decision` branch forwards four string fields
unchanged.

### §24.2 Zones

Five zones, declared most-exposed-first so `max` is the stricter reading
(`Zone::strictest`, and `ORDER` in `zones.js`). The three opaque zones are
`Assessment | External | Unknown`. `ASSESSMENT_HINTS` is exactly `quiz`,
`assessment`, `exam`, `proctor`, `lockdown`; `EXTERNAL_HINTS` is
`tool_content`, `lti`, `external_tool`, `basic_lti`; `KNOWN_FRAME_IDS` is
`preview_frame`, `wiki_page_show`, `speed_grader_iframe`; anything else
returns `"unknown"`. A frame is read as `id`, `name`, `src`, `title`,
`className` joined and lower-cased :  never by reading its contents. The twelve
page kinds match `PageKind`, and `classify_route` accepts numeric ids only
(`a_route_id_is_always_numeric`).

Both ends classify, and the host's `Broker::zone_of` is exactly what §24.2's
last paragraph says: with no parsable sanitized URL, an already-opaque
observation stands and anything else reads `Unknown`; otherwise the
extension's zone is `strictest` with the route's.

### §24.3 The account probe

`content.js` `probeAccount` is the one request: `location.origin +
PROBE_PATH`, where `shared.js` sets `PROBE_PATH = "/api/v1/users/self"`, with
`redirect: "error"`, `credentials: "same-origin"`, `cache: "no-store"`,
`Accept: application/json`. `response.redirected` or `!response.ok` returns
`null`. The body is reduced to `{ user_id, observed_at }` in the content
script. `Broker::verify` drives the `account_mismatch` rule and
`Broker::update` erases, pauses and refuses on a failure.

### §24.4 Text release

`CAPABILITY_PARAMS` is exactly the eight §24.4 names, matched after
`to_ascii_lowercase`, plus the `x-amz-` prefix. `sanitize_url` keeps `http`
and `https` only, clears the query pairs it drops, removes an emptied query,
clears the fragment, and clears username and password.
`MAX_PAYLOAD_BYTES = 64 * 1024`; `bound_extract` spends the budget on the
selection first and `truncate_utf8` walks back to a character boundary, with
`truncated` sticky.

### §24.5 The native host and the broker

`serve()` runs in the order §24.5 gives: `check_caller` before a byte is read
from the pipe, `IdentityLock::acquire_shared`, `Ownership::take` (a second
host reports `pid` and `started_at` and returns `HostError::Refused`, exit 8),
`clear_stale_socket` under that lock, `bind` then `restrict_socket`, then
`Ready`. `is_extension_id` is 32 bytes in `a..=p`. `IDENTITY_POLL` is two
seconds and the same watchdog calls `push_panel` when `log_moved`.

`Endpoint::for_identity` gives `<data root>/bridge/<key>.lock` and `.sock`;
`DIR_MODE` is `0o700`, `SOCKET_MODE` `0o600`; `pipe_name` is
`\\.\pipe\canvas-cli-<key>`; `pipe_sddl` is
`D:P(A;;GA;;;{owner})(A;;GA;;;SY)`; `MAX_SOCKET_PATH` is 103 and `bind`
refuses over it with a message naming `CANVAS_DATA_ROOT`.

`release::request` sends `Op::Release` and waits `RELEASE_TIMEOUT` = 5 s,
reporting busy otherwise; `commands/identity.rs:149` adds "stop it with
`canvas bridge detach`". `forget_endpoint` unlinks the socket **and** the
ownership lock and nothing else :  its test asserts the directory, a
neighbour's lock and the root identity lock survive.

### §24.6 `bridge-native@1`

4-byte native-endian length (`from_ne_bytes` / `to_ne_bytes`),
`MAX_MESSAGE_BYTES = 1 MiB` checked against the header before the body vector
exists. The message table is complete in both directions: ten
`ExtensionMessage` variants and eight `HostMessage` variants, matching the
table row for row. Every `Broker::update` clause in §24.6 is in
`state.rs:200-272`, including that an equal generation with a different
document id counts as a new document, and `attach` replacing the one
attachment.

### §24.7 `bridge-ipc@1`

`MAX_REQUEST_BYTES = 64 * 1024`, applied by `read_bounded_line` while the line
is read. Seven `Op` variants in the SPEC's order. Thirteen `Reason` values
with the SPEC's exact spellings; `is_refusal` excludes `ZoneOpaque` alone.
`NAVIGATE_TIMEOUT` is two seconds, which is what `navigation_timeout` means.
There is no approval operation, and
`tests/m7b.rs::no_forged_approval_moves_a_plan` sends a well-formed
`{"op": "approve"}` line and asserts `refused` / `protocol` with the plan
still `prepared`.

### §24.8, §24.9 Consumers and commands

`ANONYMOUS_CONSUMER` is `"mcp"` and `consumer_of` formats `mcp:<client name>`.
The seven-command table matches the CLI. `HostManifest::new` writes exactly
the five fields §24.9 prints, with `description` `"canvas-cli companion
broker"`, `type` `"stdio"` and one `allowed_origins` entry. The macOS and
other-Unix directories match `native_messaging_dir`; Windows returns `None`
and `bridge install` answers exit 8 with a registry message. No id and no
`bridge.extension_id` is exit 2. `write_manifest` sets `0o600`.

### §24.10 to §24.16

`TEXT_PREVIEW_CHARS` is 512, `MAX_JOURNALS` 20, `PanelState.ttl_ms` is
documented and served as `0`. `panel::cursor` reports `CursorCheck::Resync`
rather than restarting. `panel::api` reads the local cache only.

`MAX_NOTE_BYTES` 8 KiB, `MAX_SOURCE_REFS` 16, `MAX_NOTES` 32,
`MAX_INLINE_DEPTH` 4, `MAX_QUOTE_DEPTH` 6 :  and `check()` maps each breach to
the reason §24.11 gives, with the 33rd note refused and no older note evicted.
`is_allowed_ref` implements the `canvas://` and `https`-on-the-granted-origin
rule including the empty-username and empty-password test, with the
`Url::origin()` reasoning in its own comment. `markdown.js` emits eleven node
types covering §24.11's list; a blocked link renders `line-through` with
" (link removed)" in `panel.js:276`; there is no image node.

`NAVIGATE_SETTLE_MS` is 10 s and `finishNavigation` is only ever called with
`"loaded"` or `"unknown"`, so the shipped companion never reports `failed`.
`may_follow` runs before `HostMessage::Navigate` is sent.

`panel::apply_decision` runs the four checks in §24.13's order, selects the
row by a `constant_time_eq` handle rather than by plan id, and names all seven
refusals. `record_plan_decision` writes the plan id and the decision and
nothing else. `awaiting_decision` has no kind filter, and `PanelPlan` has no
operation field :  which is item 45, verbatim true, including "assignment 0"
and "`discussion_reply` · course 101" from `panel.js:94-95` and
`insert_operation`'s `assignment_id: 0`.

The five `context.*` tools carry the effects and `idempotentHint` values
§24.14 gives. `docs/bench.md` at commit `4a710f6` records the `here` p50 of
0.026 ms against a 100 ms target, the follow acknowledgement at 1.234 ms
against 300 ms, and 673 bytes on the wire. `tests/bridge.rs`
`nothing_on_the_wire_carries_a_secret` plants a capability parameter and a
secret and scans both halves of the wire, exactly as §24.16 says.

### §25

Five commands, their `POST` routes and their prepare `GET`s all match
`operations/{prepare,execute,reconcile}.rs`. `admission_name` gives
`topic-<tid>`, `conversation-new-<plan id>` and `conversation-<id>`.
`read_thread` picks the replies route for a `--to` reply and returns an
incomplete empty readback for an `inbox_send` Canvas never named, with
`auto_mark_as_read=false` on every conversation read. `--offline` is exit 2
for `reconcile` (`require_client`) and the stored journal for `status`.

Four `PlanKind` variants; `MAX_TEXT_BYTES` is 1 MiB and is `submit`'s own
constant; `MAX_ATTACHMENTS` is 10. `Transform::as_str` is `text-to-html` and
`plain`. `CanonicalPlan` carries `operation`, and `OperationPlan` carries
`body.outbound_bytes` and `attachments[].path`, so §25.2's and §20's digest
correction is right. `insert_operation` stores `assignment_id: 0`, a default
payload, no file paths, `baseline_attempt: 0` and default observations.

`STATE_0004` creates `operation_journal` with the seven-state `CHECK`, the
unique index `operation_journal_plan ON operation_journal(plan_id)`, indexes
on `(state, created_at)` and `(course_id)`, and `ALTER TABLE plans ADD COLUMN
operation_json TEXT`; `STATE_USER_VERSION` is 4. The execute order in §25.4
matches `execute()` step for step, `CONTENTION_WAIT` is 5 s, and expiry is
`guard_admission` only. `record_operation_state` writes
`dataset: "operation_journal"`, the journal id as `entity_key`,
`{"state": …}` on both sides and the dedupe key
`operation:<journal_id>:<state>`. `epoch_scopes` matches both epoch tables.
`upload_user_file` sends `parent_folder_path` `"conversation attachments"` and
`on_duplicate: "rename"` to `/api/v1/users/self/files`.

`recover_owned`'s table is §25.5's table. `ASSUME_AFTER` is 30 minutes, and
`assume_not_posted` is refused for a visible candidate, an under-age journal,
and :  in `reconcile::run` :  an incomplete readback, each with its own warning
on an exit-9 envelope. A live owner gives `Verdict::NotRead` with a warning
and no state change.

`Attribution` has the four values with §25.6's meanings; `delivery` is
`observable` for a discussion reply and `not_observable` for both inbox
writes; `match_of` refuses a candidate another user authored and keeps one
with no author. `ResponseRecord` is exactly the eight allowlisted fields.
`outcome_of` gives §25.7's exits, with `Outcome::Recovery` on 9.

`list_journals` appends operation journals with the course filter applied;
`pending_operations` has the three targets, matches `inbox_send` for any
conversation, and drops an acknowledged `outcome_unknown`. `OperationResult`
and `OperationReconcileResult` match their Appendix D rows field for field,
and `tests/e2e/schema.rs::the_plan_fields_are_nullable_in_every_shape_that_carries_them`
carries the `plan@1` / `operation@1` exception §25.9 describes.

### Cross-cutting tables

- **§5.** The twelve command forms exist; `bridge *`, `note` and
  `open --follow` are B, `here` is C, the three writes and both operation
  reads are D. The reserved-name line now holds only `grades
  estimate|what-if|target`, `dashboard` and `submit --resume`, which have no
  contract in the document.
- **§7.** `Commands::has_raw_output` returns true for `Bridge { Host }`, so
  `bridge host` rejects `--json` with exit 2 like the rest of that list.
- **§9.** The endpoint, ownership-lock and journal-lock rows match
  `endpoint.rs` and `admission_name`. `bridge.extension_id` has no default;
  `bridge.pause_hidden_after` defaults to `"10m"`, accepts `<n>h|m|s` and
  rejects zero (`filter(|ms| *ms > 0)`). `operation_journal` is a
  `state.sqlite` table, so `cache clear` cannot reach it.
- **§10.** Migration row, `STATE_USER_VERSION` 4, the `0` in
  `plans.course_id`/`assignment_id`, the epoch table and the pending hook all
  verified above.
- **§14.** Twenty-four reasons: the four v1 ones, the thirteen `Reason`
  values, and the seven prepare refusals. `zone_opaque` is the one that is
  exit 0. The `initial_post_required:` message prefix belongs to the M8-a
  reads and the `details.reason` to the M8-b write, which is what
  `check_topic_writable` and the M8-a read path do.
- **§21.2.** The catalog is 43 tools in the order
  `the_catalog_is_the_report_catalog` pins, and the effect counts are exactly
  26 / 10 / 4 / 3. `asks_for_approval` is `name.ends_with(".execute")`, which
  is the catalog-driven guard §21.2 and §25.10 describe.
- **§21.3.** Six workflow files, `WRITES` and `SUBMISSION` confined to their
  own workflows, and the catalog diffed both ways.
- **§22.2 and §22.4.** Fifteen `EventKind` values in the SPEC's order and nine
  `group()` names in the SPEC's order. The plan-decision event's dataset,
  scope fallback, payload and dedupe key match `record_plan_decision`.
- **§23.7.** `canvas schema discussion` now reports
  `result_source: "result type"`, so the replaced paragraph is right.
- **Appendix A.** Every version in the table was compared against
  `Cargo.lock` by name: all match, with `toml` `1.1.5+spec-1.1.0` and
  `markup5ever_rcdom` `0.38.0+unofficial` being build-metadata suffixes of the
  stated versions. The three doubled crates are exactly `toml`, `sha2` and
  `getrandom`, as the closing paragraph says. The tokio row is right:
  `net` is a production feature of `canvas-cli` and a dev-only one of
  `canvas-api`.
- **Appendix C.** The three package rows and their review files exist.
- **Appendix D.** `Note`, `Follow`, `Operation*`, `here@1`, `note@1`,
  `follow@1`, `bridge@1`, `operation@1` and `operation_reconcile@1` match
  their result types; the `Journal`, `receipt@1` and `plan@1` nullability
  changes match `PanelJournal`/`OperationResult`/`PlanRow` and
  `insert_operation`.

### §19

Items 1-43 are verbatim against `main` apart from the two appended
"resolved by" notes. Both notes name real commits and both were verified by
running the binary built in this worktree, not by reading the commits:

- `5c171ff` exists and touches `output/json_schema.rs` and
  `output/registry.rs`. `canvas schema discussion` prints
  `"result_source": "result type"`, and
  `every_fixture_satisfies_its_own_schema` checks every registered fixture
  against its own document.
- `d18bcfa` exists and touches the registry, the generator and
  `tests/schema_cmd.rs`. `canvas schema --list` prints 57 tab-separated rows
  of command, schema and kind, including `inbox show → canvas-cli/conversation@1
  → command` and `inbox unread-count → canvas-cli/inbox_unread@1 → command`,
  with exactly five `document` rows: `error`, `event`, `follow`, `plan`,
  `receipt`. `canvas schema "inbox show"` exits 0 and its document's
  `command` field is `inbox show`.

Items 27 and 28 are still open and untouched, as pass 1 left them. Items 44
and 45 are true of the code, with the one clause corrected above. §19 now runs
1-46.

## What I could not verify

- **Anything in a real Chrome.** §24.16 and `docs/companion.md`'s check table
  are the record, and this review neither loaded the extension nor wrote into
  any Chrome profile, as the brief requires. Every Chrome-facing sentence in
  §24 was checked against `extension/` and the host's tests, which is what
  §24.16 itself claims and no more.
- **Windows.** The pipe name, the SDDL and the `bridge install` refusal are
  unit-tested on this machine; no pipe and no registry key exists to check.
- **The bench numbers.** §24.16 and §21.2 quote `docs/bench.md`, whose header
  names commit `4a710f6`. The numbers were read from that file, not
  re-measured. `docs/bench.md`'s own last commit is `60bd3ff`, which
  re-recorded the §13 table but left the catalog and bridge rows unchanged.
- **REPORT intent.** Where a sentence attributes a requirement to
  `docs/agent-ux/REPORT.md` (§3.2's trust boundary, §3.3's permissions,
  §3.4's overlay refusal, §3.5's "exact bytes"), the attribution was taken as
  the package's, not re-derived. What was checked is that the code says what
  the SPEC says it says.
- **`docs/writes-v2.md`'s "Left for the next round".** Its one-line scope note
  :  group discussions, marking read, editing or deleting a post, and quizzes
  are out of this package :  is not carried by §25 beyond the `group_write`
  refusal. Left as it is: that is roadmap, which §5 and §18 hold, not a
  contract sentence. Worth Rolf's eye rather than a correction.

## Gates

Run with `CARGO_TARGET_DIR=<checkout>`.

```
$ cargo fmt --all --check
FMT CLEAN

$ cargo nextest run --all-features
Summary [ 103.267s ] 923 tests run: 923 passed (1 slow), 0 skipped

$ git status --short
(empty)
```

923 is the count this package started from, which is the proof this review
touched no code.
