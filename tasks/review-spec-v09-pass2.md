# Review — SPEC v0.9 consolidation pass 2, branch `lane/w3` (Claude Opus 5 high)

You are the reviewer of a **documentation** package. Worktree
`/Users/rolfie/projects/canvas-cli/.worktrees/w3`, branch `lane/w3`, package
brief `tasks/spec-v09-pass2.md`. Use
`CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/rev-spec2`.

First run `git merge main` (expect nothing to do). Then read
`docs/SPEC-CHANGES-v0.9.md` and `git diff main...HEAD -- docs/` in full.

The package's only rule was: every normative sentence added to `docs/SPEC.md`
must be true of the code on `main`, no new decision may be made, and no §19
item may be resolved except by a one-line "resolved by …" note that names the
commit or review that fixed it. Your job is to check that, sentence by
sentence, against the code and its tests — not against the REPORT, the briefs,
or the worker reports. This pass wrote **§24 (companion, broker, presence,
side panel, notes, follow, panel approvals)** from M7-a and M7-b and **§25
(discussion and inbox writes)** from M8-b, re-verified Appendix A, B and D, and
reconciled §19 items 1–43. For every sentence in §24 and §25 and every changed
row elsewhere: open the module, the manifest, the registry entry, the fixture,
the test, or the extension file that proves it. Pay special attention to:
`extension/manifest.json` permissions (`activeTab`, `nativeMessaging`,
`scripting`, `sidePanel`; no `host_permissions`, `tabs`, `webNavigation`,
`storage`, `cookies`); the `bridge-native@1` and `bridge-ipc@1` message sets
and their `reason` values against `canvas-core::bridge`; the lock and socket
paths and modes against the broker code; `here@1`, `bridge@1`, `note@1`,
`follow@1`, `operation@1`, `operation_reconcile@1` field lists against the
registry fixtures and snapshots; every exit code named in §24/§25 against the
tests; the plan kinds, migration `0004_operations`, attribution values, and
the `channel: panel` approval record against `canvas-core::plan` and
`crates/canvas-cli/src/bridge/panel.rs`; the MCP tool names and annotations
against the catalog test (count them); Appendix A against `Cargo.toml` and
`Cargo.lock` (tokio `net` must now be production, and rustix, rmcp, schemars,
uuid rows must match); the `docs/companion.md` and `docs/writes-v2.md`
pointers must not drop any manual-check table or "not run in real Chrome"
statement that §24 does not carry; and every §19 "resolved by" note must name
a real commit or review file and the item text must otherwise be verbatim.

Fix every false, invented, or ambiguous sentence yourself in separate
`review(SPEC-v0.9):` commits — by correcting it to what the code does, or by
deleting it when the code proves nothing either way. If the code is wrong and
the sentence is right by the REPORT, do **not** change the code: leave the
sentence describing the code and add a §19 item naming the gap. Never push,
never merge into `main`, touch no code and no tests, install nothing into the
owner's Chrome profile, and touch no identity or credentials.

Write `docs/reviews/spec-v0.9-pass2.md`: the verdict (MERGE or REWORK), a table
of what you corrected (section, what was wrong, what it says now, the evidence
file), what you checked and found true, and what you could not verify. Run
`cargo fmt --all --check` and `cargo nextest run --all-features` at the end to
prove no code changed (expect 923 passed). Finish with `git status --short` and
reply exactly on its own line: `DONE docs/reviews/spec-v0.9-pass2.md`

The worker's final report, for reference:

```
# SPEC v0.9 consolidation, pass 2 — final report

Lane `w3`, worktree `/Users/rolfie/projects/canvas-cli/.worktrees/w3`, branch
`lane/w3`. Brief: `tasks/spec-v09-pass2.md`. Docs only; no code was touched.

Eight commits, four files changed, 807 insertions and 971 deletions.

```
9e08a05 docs(spec): correct the broker lock's removal and escape the plan@1 row
02d39fa docs(spec): record pass 2 in the changes file and drop "draft"
255922f docs: reduce companion.md and writes-v2.md to pointers
d66b4bb docs(spec): reconcile §19 with the code on main
de4a103 docs(spec): re-verify Appendix A and extend Appendices B, C and D
6d78daf docs(spec): correct the §20-§23 sentences the three merges falsified
6dbb0ba docs(spec): extend §5, §7, §9, §10 and §14 for the companion and the writes
60a001a docs(spec): write §24 and §25 from M7-a, M7-b and M8-b as built
```

```
 docs/SPEC-CHANGES-v0.9.md | 129 ++++++++-
 docs/SPEC.md              | 663 ++++++++++++++++++++++++++++++++++++++++++----
 docs/companion.md         | 573 +++------------------------------------
 docs/writes-v2.md         | 413 +++--------------------------
```

Method: every normative sentence was checked against the code on `main` and its
tests, not against the package reports. Where the code and a report disagreed,
the code won and the difference is recorded (see "What the code contradicted in
a report"). No §19 item was resolved, reworded, or renumbered by me.

---

## 1. Sections written

### §24 Companion, broker, presence

Written from M7-a and M7-b as built. Sixteen subsections:

| Subsection | What it records |
|---|---|
| 24.1 The extension | Manifest V3, `minimum_chrome_version` 116, and **exactly four** permissions — `activeTab`, `nativeMessaging`, `scripting`, `sidePanel` — with what the manifest does **not** ask for (`host_permissions`, `content_scripts`, `externally_connectable`, `cookies`, `webRequest`, `declarativeNetRequest`, `webNavigation`, `history`, `tabs`, `storage`, `downloads`, `debugger`), the test that pins all of it, the gesture and attachment lifecycle, the navigation generation, the five pause causes, and the rule that the service worker performs no fetch and reads no page |
| 24.2 Zones | The five zones, the stricter-of-two rule, the three opaque zones and `content_reason: zone_opaque`, frame classification by attributes only, the assessment and external hint lists, Canvas' own three known frame ids, the rule that an unrecognized frame makes the whole page `unknown`, and that **both ends classify** — with the host taking the stricter reading and an opaque page sending no URL at all |
| 24.3 The account probe | The one fixed request `GET <origin>/api/v1/users/self`, why it runs in the isolated-world content script, its four request options, the reduction to `{ user_id, observed_at }` before anything leaves the page, and the `account_mismatch` rule |
| 24.4 Text release | Metadata from broker memory only; text only on a consumer's ask and only after a fresh probe; the eight stripped capability parameters plus the `x-amz-` prefix; what `sanitize_url` drops; the 64 KiB UTF-8 bound cut on a character boundary |
| 24.5 The native host and the broker | The five-step `serve()` order and what each step refuses; the two-second identity watchdog; the endpoint, its `0700`/`0600` modes, the Windows pipe and its DACL, the 103-byte path check; and the `identity remove` cooperative release |
| 24.6 `bridge-native@1` | Native-endian framing, the 1 MiB bound enforced from the length alone, the full nine-by-eight message table, the pause causes, and what `Broker::update` decides for each kind of mismatch |
| 24.7 `bridge-ipc@1` | The 64 KiB request bound and why it is larger than the note bound; the seven-operation table with who may call each; **there is no approval operation on this socket**; and the thirteen refusal reasons with `zone_opaque` marked exit 0 |
| 24.8 Consumers and the trust boundary | The consumer naming scheme, the explicit statement of §19 item 31's boundary, and the four things that do hold above the socket |
| 24.9 Commands | The seven-command table with classes; the manifest template with the description string the code writes; the platform directories; Windows refused with exit 8; and why `here` is class C while the rest are class B |
| 24.10 The side panel | No model, no database, no request of its own; the API side as whole §7 envelopes read **offline and only offline**; the status feed's cursor rules, the 20-journal bound, and the rule that only `submitted` is drawn as done |
| 24.11 Notes | The three things a note is not; the five bounds table (8 KiB, 16 refs, 32 notes, emphasis 4, quotes 6) and what breaking each costs; the source-ref rule including the credentials case; and why the count is bounded twice |
| 24.12 Follow | Dispatch acknowledgement versus load outcome; **this build never reports `failed`**; the ten-second timeout; per-consumer ownership; navigation is not a preview; and the two-way generation rule |
| 24.13 Panel approvals | The three decision words; the host's four ordered checks including constant-time handle comparison; the six refusal names; that a page script cannot reach any of it; the private approval events; and the §19 item 45 gap |
| 24.14 The agent surface | The five `context.*` tools with effects and idempotency; the `/context` resource and what a foreign handle reads; and `ttl_ms: 0` |
| 24.15 Schemas | `here@1`, `note@1`, `follow@1`, and `bridge@1` with its three variants |
| 24.16 What has been run, and what has not | What is tested against the shipped host, under Node, and by the benchmark — and, plainly, the fourteen things never run in a real Chrome, plus Windows |

### §25 Discussion and inbox writes

Written from M8-b as built. Ten subsections:

| Subsection | What it records |
|---|---|
| 25.1 Contract | The five-command table with every request and admission lock name; the four prepare reads; the three readback routes and why a threaded reply uses the replies route; and the `--offline` split between `status` and `reconcile` |
| 25.2 The plan | The three plan kinds on the §20 layer; what a plan freezes; the 1 MiB and 10-attachment limits; how an operation plan is stored (`0` in the two submission columns, `plans.operation_json` carrying the target); the two body transforms and why each; and **what the digest covers**, including the §20 correction |
| 25.3 Refusals at prepare | The seven `details.reason` values with their exact conditions; the cross-origin exit-6 exception; the initial-post gate is never opened; and why a discussion reply cannot carry an attachment |
| 25.4 The operation journal | Migration `0004_operations`; the seven states; the four discipline rules including **the admission difference from §12.2** (§19 item 39); why `inbox_send` locks on the plan id; the full execute order; uploads; the `operation.state` event; and the cache epoch table |
| 25.5 Recovery and ambiguous outcomes | The owner-absent recovery table; nothing is ever resent; why a 5xx is `outcome_unknown` and a 4xx is `failed`; the three conditions that refuse `--assume-not-posted` and the residual-risk statement; and "a live owner stops recovery, not reading" |
| 25.6 Attribution | The four-row ladder; why a digest match is `unproven` and never `observed`; `delivery` as a separate field; **a conversation Canvas accepted is not delivered mail**; and the eight-field response allowlist |
| 25.7 Exits | The five-row exit table; exit 9 is `outcome: recovery`, "ask again", never "it failed" |
| 25.8 Receipts and the pending hook | One `Journal` shape for both kinds with `kind` telling them apart; which fields are `null` on an operation row; `superseded` always false; both id spaces searched; and the pending rules with the three targets |
| 25.9 Schemas | `operation@1`, `operation_reconcile@1`, the additive blocks on `plan@1`, `Journal` and `receipt@1`, and why `plan_id` is never null on the two schemas that name their own plan |
| 25.10 The agent surface and the skill | The eight tools with effects; the elicitation round trip and the no-elicitation refusal; `replayed: true`; the catalog-driven retry guard; and the sixth workflow with the REPORT §3.5 course-policy boundary |

---

## 2. Tables extended

| Section | What changed |
|---|---|
| §5 Commands added after v1 | Twelve new command forms added |
| §5 Reserved names | `bridge …`, `here`, and the write halves of `inbox` and `discussion` dropped — they now have contracts. `grades estimate\|what-if\|target`, `dashboard`, `submit --resume` stay reserved |
| §5 Behaviour notes | Three new lines pointing the new commands at §24 and §25 |
| §5 Command classes | **B** gains `bridge install\|host\|status\|detach`, `note`, and `open --follow`; **C** gains `here`; **D** gains `discussion reply`, `inbox send`, `inbox reply`, `operation status`, `operation reconcile`. A closing paragraph explains why `here` is C and names §19 item 37's `operation status --offline` exception |
| §7 Output contract | `bridge host` joins the raw-output list |
| §9 Paths | Broker endpoint row loses "reserved" and gains the Windows DACL; new row for the broker ownership lock; the journal lock row names `topic-<tid>.lock`, `conversation-<id>.lock`, and `conversation-new-<plan-id>.lock` beside `assignment-<id>.lock` |
| §9 Config | A `[bridge]` block in the example, and a table of the two keys — `bridge.extension_id` (no default) and `bridge.pause_hidden_after` (`10m`, zero rejected). The "no `bridge.*` key exists yet" sentence is gone |
| §9 State tables | `operation_journal` joins the list `cache clear` cannot reach |
| §10 Migration list | `0004_operations` added with its unique index, its two indexes and `plans.operation_json`; `STATE_USER_VERSION` 3 → 4; a note on the `0` in `plans.course_id`/`assignment_id` |
| §10 Mutation epochs | A table of the three operation kinds and the scopes each bumps |
| §10 Pending hook | Extended with operation journals, the no-superseding rule, and the three pending targets exactly as `pending_operations` computes them — including that `conversation@1` also reports every unresolved `inbox_send` |
| §14 Refusal reasons | Four reasons → twenty-four: thirteen companion reasons and seven prepare refusals, plus a note that `zone_opaque` is exit 0 and a sentence separating M8-a's `initial_post_required:` message prefix from M8-b's `details.reason` |
| §20 | A paragraph naming the three M8-b kinds on this layer, plus the `plan_sha256` correction |
| §21.2 | 30 tools → **43**, effect table rebuilt and counted (26 Read, 10 Organize, 4 RemoteWrite, 3 Retire); the four `*.execute` tools replace the single-name approval guard; the `context/{consumer_handle}` row says what it serves; catalog size re-measured |
| §21.3 | Five workflows → **six** |
| §22.2 | `operation.state` and the three plan-decision events with datasets, scopes, dedupe keys and payloads; eleven kinds → **fifteen** |
| §22.4 | Seven notify groups → **nine** (adds `operation` and `plan`) |
| §23.7 | The "no typed arm" paragraph replaced: the eight M8-a schemas now derive from their result types |
| Header | `draft v0.9` → `v0.9`; the placeholder sentence replaced |

---

## 3. Appendix corrections

### Appendix A — re-verified against `Cargo.lock`

**Every version in the table matches the lock file exactly.** Checked
individually: clap 4.6.6, clap_complete 4.6.9, clap_mangen 0.3.3, tokio 1.53.1,
reqwest 0.13.5, futures-util 0.3.34, serde 1.0.229, serde_json 1.0.151, jiff
0.2.35, keyring 4.2.0, etcetera 0.11.0, figment 0.10.19, toml 1.1.5, rusqlite
0.40.2, cap-std/cap-fs-ext 3.4.6, fs4 0.13.1, comfy-table 8.0.0, anstyle 1.0.14,
anstream 1.0.0, indicatif 0.18.6, open 5.4.3, sha2 0.10.9, htmd 0.5.5,
markup5ever_rcdom 0.38.0, tracing 0.1.44, tracing-subscriber 0.3.23, uuid
1.18.1, thiserror 2.0.20, anyhow 1.0.104, rmcp 3.2.0, schemars 1.2.2, and the
dev and tool rows.

Changes made:

- **tokio row rewritten.** `net` is now a **production** feature of `canvas-cli`
  (`["rt","macros","fs","io-util","net","sync","time","signal"]`) since M7-a,
  because the broker's Unix socket and named pipe need it. `canvas-api` still
  enables it in dev-dependencies only. The old row said `net` was dev-only and
  pointed at M7-a as future work.
- **`rustix` note extended** to say what it is for under `cfg(unix)`: the socket
  and pipe permission work of §24.
- **"No post-v1 package added a dependency after `rmcp` and `schemars`"** made
  explicit for M7-a, M7-b and M8-b, with the fact that the companion ships as
  plain JavaScript with no npm dependency at all, and that M7-a's only manifest
  change was the tokio feature.
- **`uuid`'s roles** now include operation journal ids.
- **New closing paragraph** recording the re-verification and naming the three
  crates the lock file carries twice for transitive reasons: `toml` (1.1.5
  direct, 0.8.23 transitive), `sha2` (0.10.9 direct, 0.11.0 transitive), and
  `getrandom` (three entries).

### Appendix B

- New table for the three writes: prepare reads, both `POST` routes per kind
  with their bodies, the attachment upload route (`parent_folder_path` =
  `conversation attachments`, `on_duplicate` = `rename`), and the readbacks.
- Note that `group_conversation` is sent as `false`, always.
- New row: `here` adds no endpoint; its API half calls the `course`,
  `assignment` and `announcement` cores.
- New table for the one Canvas request the companion makes — from the
  extension's content script, same-origin, with the browser's own cookies — and
  the statement that no token of this CLI is ever sent through the browser and
  no request is ever proxied through it.

### Appendix C

- M7-a, M7-b and M8-b added to the packages table with their sections and
  review files.
- "Still in flight, and not on `main`: M7-a and M7-b … and M8-b" replaced by:
  every post-v1 package REPORT §4 scheduled is now on `main`. M8-c (GraphQL)
  and M8-d (OAuth) stay conditional.
- The fate of all three contract documents recorded, including the one thing
  `docs/companion.md` keeps that no section replaces.

### Appendix D

New shared objects: `Note`, `Follow`, `Operation`, `OperationTarget`,
`OperationAttachment`, `OperationResponse`, `OperationReadback`,
`OperationMatch`.

New schema rows: `here@1`, `note@1`, `follow@1`, `bridge@1` (three variants),
`operation@1`, `operation_reconcile@1`.

Nullability corrected to what the code produces:

- **`Journal`** gains `operation?`, and `course_id`, `assignment_id`,
  `assignment_name` and `baseline_attempt` become nullable — they are `null` on
  an operation row, along with `posted`, `readback` and `server_match`.
- **`receipt@1`** gains `operation?`.
- **`plan@1`** gains `operation?`, makes `course_id` and `assignment_id`
  nullable, and lists all seven plan kinds.
- **`discussion@1`, `inbox@1`, `conversation@1`, `inbox_unread@1`** gain
  `pending: bool` and `pending_journals: [id]`.

Three rows added to the "fields added after v1" table.

One rendering fix: the `plan@1` row carried unescaped `|` inside its code span,
so GFM split the cell; this pass added six more with the plan kinds, and every
pipe in that cell is now `\|`. Seven other pre-existing rows with the same
problem were left alone — this pass did not add pipes to them.

---

## 4. §19 reconciliation

§19 now runs 1–45. **No existing item was resolved by me, reworded, deleted, or
renumbered.**

### Resolved by the code — one line appended to each, original text untouched

| # | The item | Note appended |
|---|---|---|
| 35 | The eight M8-a schemas have no typed arm in the schema generator, so `canvas schema discussion` reports `message_markdown` as `"type": "string"` although §7 lets it be `null` | **"Resolved by 5c171ff (M8-b round): the eight result types derive `JsonSchema`, their pages declare `result_source: "result type"`, and every registered fixture is checked against its schema."** |
| 36 | `canvas schema --list` derives command names from schema ids, so it names commands that do not exist, and `canvas schema "inbox show"` exits 6 | **"Resolved by d18bcfa (M8-b round): a registry entry carries the command that prints it, `canvas schema --list` prints command, schema, and kind, and `canvas schema "inbox show"` and `canvas schema "inbox unread-count"` now resolve; the five entries no command prints are listed as `document`."** |

Both were verified by **running the binary**, not by reading the commits:

- `canvas schema --list` prints 57 rows in three tab-separated columns, with
  `inbox show → canvas-cli/conversation@1 → command` and
  `inbox unread-count → canvas-cli/inbox_unread@1 → command`, and exactly five
  `document` rows (`error`, `event`, `follow`, `plan`, `receipt`).
- `canvas schema "inbox show"` exits 0 and prints a document whose `command`
  field is `inbox show`.

An apparent contradiction was traced and resolved: pass 1 asserted item 36 was
still true, yet `d18bcfa` fixes it. Merge ancestry shows `d18bcfa` and `5c171ff`
reached `main` in the M8-b merge `4f7acda`, which is **after** the pass-1 merge
`9318f0e`. Pass 1 was correct when written.

**Items 27 and 28 were deliberately left open.** Their fixes predate pass 1,
which read them and left them for the owner; nothing since has changed that.

### Added — new REPORT-versus-code differences

| # | Difference |
|---|---|
| 44 | **The companion declares `sidePanel` as a fourth permission.** REPORT §3.3 names `activeTab` and `nativeMessaging`; item 30 recorded `scripting` as the third and was closed on that reading, **before M7-b added `sidePanel`**. It grants no host access, no tab access and no way to read anything, and REPORT §3.4 forbids the alternative of a Canvas DOM overlay — so the same reasoning applies, but item 30's confirmation cannot cover a permission added afterwards. `tests/companion.rs` pins the four names exactly. |
| 45 | **The panel draws an operation plan without its body.** `plan::awaiting_decision` filters by plan state and handle only, so every waiting plan reaches the panel, whichever kind it is; `PanelPlan` carries the submission half (`files`, `text_preview` from `payload.text`, the baseline attempt) while an operation plan stores its body, thread and recipients in `operation_json`. Such a row draws as `assignment 0` with `discussion_reply · course 101` (or `course 0` for an inbox write), its digests and its expiry — and with no message text, no topic and no recipients. Approving it there still works and every host-side check is unchanged, so it is **not** a forgery path; but REPORT §3.5 requires the exact bytes before an approval and this surface cannot show them. |

Item 45 is the visible seam between two lanes that landed in the same round:
M7-b's panel and M8-b's plan kinds are each correct alone, and no test drives
one through the other.

---

## 5. The pointers

### `docs/companion.md` — 613 lines → 106

Points at §24 for the contract, and at §9, §5, §14, §22 and Appendix D for the
rest of the surface. Names §19 items 30, 31, 32, 41, 42, 43, 44 and 45 as the
open ones, and links both reviews.

**Two things stay**, because no SPEC section carries them:

1. The **install walkthrough** — the six numbered steps and the two `config set`
   lines. `README.md:219` links to this file for the install steps, and README
   is not editable in this pass.
2. The **"How to run the Chrome checks yourself"** table — seventeen rows.
   Nothing in that package has been run in a real browser, so this table is
   still the only way anyone finds out whether it works.

Removing the manifest template also removed a document-versus-code difference
(see below).

### `docs/writes-v2.md` — 382 lines → 31

A pure pointer at §25, with cross-references to §20, §10, §9, §14, §22, §21 and
Appendix D. Names §19 items 37, 38, 39, 40 and 45 as the open ones, and links
the review. Nothing was dropped. Only `tasks/` still links to it, and `tasks/`
was not touched.

---

## 6. What the code contradicted in a report

Four cases. In each the code won, and each is recorded in
`docs/SPEC-CHANGES-v0.9.md` rather than quietly fixed.

1. **§20's `plan_sha256` exclusion is true of a submission plan only.**
   §20 says the digest excludes the outbound bytes and the local file paths.
   `CanonicalPlan` serializes `operation` when a plan has one, and
   `OperationPlan` carries `body.outbound_bytes` and each
   `attachments[].path` — so both are **inside** the digest for the three M8-b
   kinds. §20 now says so, and §25.2 repeats it where a reader of the writes
   will look. Nothing weaker follows: the bytes and files are still pinned by
   their own digests, and execute still re-hashes every attachment from disk.

2. **`docs/companion.md` printed the wrong manifest `description`.**
   It showed `canvas-cli browser companion broker`; `HostManifest::new`
   (`crates/canvas-cli/src/bridge/manifest.rs:87`) writes
   `canvas-cli companion broker`. §24.9 carries the string the code writes, and
   the template is gone from the pointer.

3. **`docs/writes-v2.md` recorded a 38-tool catalog.**
   True when M8-b was written; M7-b changed it. The measured catalog is
   **43 tools, 344 878 bytes, about 86 235 estimated tokens**, from
   `docs/bench.md` at commit `4a710f6`. §21.2 carries the measured numbers.

4. **`OperationReceipt.transform`'s doc comment says "`plain` or `html`".**
   The value written is `plain` or `text-to-html`, from `Transform::as_str`.
   The SPEC uses the values, not the comment.

A fifth correction was internal to this pass's own draft: §24.5 first said the
persistent lock files all stay after `identity remove`. `release::forget_endpoint`
unlinks the socket **and** the broker's own ownership lock file — the one
exception to §10's never-delete-a-lock rule, which is what §9's row already
said. Fixed in `9e08a05`.

### Also verified, and worth the owner's eye

- **The `--help` epilogue is still headed "Commands (v1)".** It has since gained
  `discussion reply`, `inbox send|reply`, `operation status|reconcile`,
  `bridge install|host|status|detach`, `here` and `note`, so the heading is now
  wrong about most of what it lists — and it still omits `pages`, `page`,
  `syllabus`, `discussions`, `discussion`, `inbox`, `schema` and `mcp`. Pass 1
  raised the omission; the heading is new.
- **Nothing in `extension/` has ever run in a browser.** The benchmark, the
  broker tests and the npm tests all exercise code that runs outside Chrome, or
  code Chrome would run — never Chrome itself.

---

## 7. The header

"draft" was dropped from the SPEC status line. The brief's test: drop it only if
every placeholder is gone and every section describes built code. Both hold.

- **Every placeholder is gone.** §24 and §25 were the only two. A search for
  "placeholder", "reserved for" and "in flight" now returns only ordinary prose:
  the reserved *command names* in §5 (names, not sections), the Markdown
  placeholder line §23.3 puts in place of embedded content, the initial-post
  rule in §25.3, and "in flight" describing a request on the wire.
- **Every section describes built code.** §§1–19 are the v1 contract, which
  shipped. §§20–25 record six merged packages, each with its own review file,
  all listed in Appendix C.

What stays open is §19, a list of questions for the owner that has been part of
the document since v0.1. It is not a placeholder and does not describe unbuilt
code. The changes file states all of this.

---

## 8. Gates

```
$ cargo fmt --all --check
FMT CLEAN

$ cargo nextest run --all-features
Summary [ 55.256s] 923 tests run: 923 passed, 0 skipped

$ git merge main
Already up to date.

$ git status --short
(empty)
```

Both run with `CARGO_TARGET_DIR=/Users/rolfie/projects/canvas-cli/.target/w3`.

**923 tests is the same count as the baseline taken before any edit**, which is
the proof this pass touched no code. Nothing was pushed. Nothing was merged into
`main`. The working tree is clean.
```
