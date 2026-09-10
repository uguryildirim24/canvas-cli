# Code review — M8-a2, the follow-on to M8-a (lane `lane/w1`)

## Verdict

**MERGE.** The merge of `main` (M6-b) into the lane is clean — every registry
entry, command enum arm, README row, and catalog name from both parents
survives, none is duplicated, and no snapshot was hand-edited. The eight MCP
read tools are built on the M6-b handler pattern, are `readOnlyHint: true`,
and call the same `handle_*` cores the CLI calls. SPEC §19 items 27 and 28
are applied as the coordinator read them. Two defects were found and fixed in
three `review(M8-a2):` commits: `replies_total` reported a count for a thread
it had never read, which is the very confusion item 28 exists to prevent, and
the tool-versus-command comparison covered all eight tools but only on an
envelope both sides reach before reading their arguments. Nothing is pushed
and nothing is merged into `main`; three items are listed under "Needs a
decision" and none of them blocks this package.

## Gates

Run with `CARGO_TARGET_DIR=/home/user/projects/canvas-cli/.target/rev-m8a2`,
first on the delivered branch and again after the fixes and a final
`git merge main`.

| Gate | As delivered | After the fixes |
|---|---|---|
| `cargo fmt --all --check` | clean | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean | clean |
| `cargo nextest run --all-features` | 670 passed, 0 skipped | 671 passed, 0 skipped |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok | same |
| `cargo +1.88 check --workspace --all-targets` | clean | clean |
| `cargo xtask bench --mcp` | every §13 target met | not rerun after the fixes |

`cargo xtask bench` rewrites `docs/bench.md`, which this package must leave
as `main` has it; the file was restored after the run and `git status` is
clean.

**Catalog size, confirmed.** `cargo xtask bench --mcp` on this branch reports
`catalog: 30 tools, 220855 bytes, ~55224 tokens per tools/list`, against the
22 tools and ~41891 tokens `docs/bench.md` records from M6-b. The worker's
number is right, and `docs/reads-v2.md` carries it. Per SPEC §19 item 19 the
catalog design is the owner's call, so nothing here changes it.

## What was checked and holds

- **The merge lost nothing.** The registry snapshot, `registry.rs`, the
  `Commands` enum, the `main.rs` dispatch arms, and the README command table
  were each diffed against both parents of `21bee73`. Parent 1 (lane) and
  parent 2 (`main`) contribute 39 and 31 schema ids, 31 and 27 enum arms, 52
  and 46 README rows; the merge holds the union of each — 39, 33, 54 — with
  nothing lost from either side and nothing duplicated. The only repeated
  schema ids in the snapshot (`receipts@1`, `config@1`, `cache@1`,
  `identity@1`) are the multi-shape entries that were repeated in both
  parents too. `output/mod.rs`, the one conflicted file, keeps every
  identifier both sides had. No conflict marker survives anywhere in the
  tree.
- **The snapshots are generated, not written.** `cargo nextest run` passes
  with insta in its default mode, which fails on any mismatch, so the
  registry snapshot and the M8-a snapshots are what the code produces. No
  stray `.snap.new` is left.
- **Every new tool is a read.** `annotations_describe_effects` walks every
  spec, so `readOnlyHint: true`, `destructiveHint: false`, and
  `idempotentHint: true` are asserted for all eight rather than for a
  sample. Each tool dispatches to the `handle_*` core its command calls —
  `pages::handle_list`, `pages::handle_show`, `pages::handle_syllabus`,
  `discussions::handle_list`, `discussions::handle_show`,
  `inbox::handle_list`, `inbox::handle_show`, `inbox::handle_unread_count` —
  so a tool cannot reach a route its command does not. Nothing in the package
  sends a verb other than `GET`: `assert_only_get` covers the M8-a request
  log, and the new comparison test asserts the same over its own fixture.
- **The eight are in all four places the brief names.** The catalog allowlist
  test (`the_catalog_is_the_report_catalog`), `tests/mcp.rs` `CATALOG` and
  `EQUIVALENTS`, `tests/skill.rs` `CATALOG`, and the skill itself. The skill
  test diffs the catalog against the workflows in both directions, so the
  command list matches the catalog by construction; the two workflows carry
  `replies_coverage`, `replies_total`, `messages_complete`, and `embedded`
  as rules an agent must not paper over.
- **§19 item 27, `--announcements`, is fully removed.** The flag is gone from
  clap, from the `main.rs` arm, from `handle_list`, from the README row, and
  from the fixture that had mounted an announcement the pinned request
  cannot return. `is_announcement` stays on `discussions@1` and
  `discussion@1`, which is right: Canvas sends it on a topic. The human
  table now names `canvas announcements`, in both the populated and the
  empty case. Nothing outside `tasks/` still mentions the flag.
- **§19 item 28 is additive.** `replies_page` and `replies_total` are new
  fields beside `replies` and `replies_coverage`; nothing is renamed,
  reordered, or retyped, the `discussion@1` fixture gains the two fields
  without changing a value, and every M8-a snapshot written before the change
  still validates. A page past the end stays exit 0, `--page 0` and `--page`
  without `--replies` stay exit 2.
- **Item 4 is correctly deferred.** `git log main` shows no `merge lane/w3:
  M6-c`, no event or watch code is in the tree, and nothing in this package
  writes an event. `docs/reads-v2.md` records it as left for the next round.
- **MSRV and dependencies.** No dependency was added or moved; `Cargo.toml`
  and `Cargo.lock` changed only by taking `main`'s side in the merge.
  `cargo +1.88 check --workspace --all-targets` is clean and `cargo deny
  check` passes on all four checks.

## Defects found and fixed

| Severity | Where | What was wrong | What I changed | Commit |
|---|---|---|---|---|
| Major | `crates/canvas-cli/src/commands/discussions.rs:229`, `crates/canvas-cli/src/output/registry.rs:1541` | `replies_total` counted the entries the command had loaded, and a read without `--replies` loads none — so a topic with three replies printed `replies_total: 0`, the same document a thread with no replies prints. That is exactly the confusion §19 item 28 was added to remove, and `a_replies_page_past_the_end_is_empty_at_exit_zero:480` asserted that very document as its "no replies" case (`discussion 5 57`, whose `discussion_subentry_count` is 3, read without `--replies`). | `replies_total` is `Option<u32>`: the covered-set count when `--replies` was asked, `null` when it was not, per §7's reading of `null` as unknown. The field stays present and nothing is renamed, so the change is still additive. The test now reads a genuinely reply-free thread *with* `--replies` for the 0 case and asserts `null` for the topic read without it. `docs/reads-v2.md` decision 12 and the skill's `read-an-assignment.md` say so. | `5d5d22b` |
| Medium | `crates/canvas-cli/src/commands/discussions.rs:690` | The human line read `replies: page 1 of 3 shown, 2 pages fetched` — "page 1 of 3" beside "2 pages fetched" asks a reader to read one number in two units, and 3 is a reply count, not a page count. | The line now says `replies: showing page 1; 3 replies in the covered set, 2 pages fetched, complete=true`, and a thread that was not read says `replies: not read; --replies asks for the thread`. Snapshot regenerated. | `5d5d22b` |
| Medium | `crates/canvas-cli/tests/mcp.rs` `EQUIVALENTS` | The comparison covers all eight tools, but it runs `--offline` against a cache primed only by `sync`, which holds none of the M8-a datasets. I instrumented the test and confirmed all eight meet their commands as `canvas-cli/error@1`, exit 7 — an envelope both sides reach before any operand past the first is read. Swapping `course` and `page` in `page.get`, or dropping `page`, `scope`, or `unread` from the dispatch, leaves that comparison passing. So the brief's requirement was met in letter, but nothing checked that a tool forwards its arguments. | Added `the_m8a_read_tools_answer_with_the_envelope_their_command_prints`: a fixture that answers the whole read surface, warms each dataset with one online command, then compares tool against command offline with every argument each tool takes — `unpublished`, `unread`, `replies`, a `page` past the end, and `scope` — and asserts `outcome: "ok"` so it cannot silently degrade into the refusal comparison again. Each of the five arguments is load-bearing in the fixture: I mutated the dispatch five times and confirmed each mutation fails the test. The request log is asserted `GET`-only. | `2afee49` |
| Low | `docs/reads-v2.md` | The document said the tools are "in the tool-versus-command comparison test" without saying what that test compares, which reads as a stronger guarantee than the offline table gives. | Named both halves of the comparison. | `8ea6deb` |

## Needs a decision

1. **`canvas schema` cannot name `inbox show` or `inbox unread-count`.**
   `canvas schema --list` derives a command name from the schema id, so
   `conversation@1` is listed as `conversation` and `inbox_unread@1` as
   `inbox unread` — neither is a command — while `canvas schema "inbox show"`
   and `canvas schema "inbox unread-count"` both exit 6. This is not M8-a2's
   doing: it is how M6-b's registry lookup works on `main`, where
   `submission reconcile` and `receipts verify` exit 6 for the same reason
   and `plan`, `reconcile`, `receipt`, and `verify` are listed as commands
   that do not exist. M8-a's two schemas simply join that set. Fixing it
   means giving `SchemaEntry` a command name of its own, which is M6-b's
   contract and another lane's file, so it is the owner's call rather than
   the reviewer's.
2. **The eight new output schemas describe nullable fields as
   non-nullable.** `page@1`, `pages@1`, `syllabus@1`, `discussions@1`,
   `discussion@1`, `inbox@1`, `conversation@1`, and `inbox_unread@1` have no
   typed arm in `json_schema::result_schema`, so their documents are inferred
   from the registry fixture — `canvas schema discussion` reports
   `message_markdown` as `"type": "string"` and `points_possible` as
   `"type": "number"` although §7 lets both be `null`. The document declares
   `result_source: "registry fixture"`, so it is honest about being an
   approximation, but a strict host validating a tool result against the
   `outputSchema` would reject a legitimate answer. My `replies_total` fix
   adds one more nullable field to that set. The remedy is either a
   `JsonSchema` derive on the M8-a result types or teaching
   `schema_of_fixture` to widen every property; both change M6-b's contract
   for other schemas too, so I left it.
3. **`docs/bench.md` now contradicts the branch.** It records 22 tools and
   ~41891 tokens from `main`, while this branch serves 30 tools and ~55224.
   The brief forbids writing under `docs/` except `docs/reads-v2.md`, so the
   worker recorded the new number there instead, which is the right call for
   the package. `docs/bench.md` needs regenerating when this lane reaches
   `main`, and the growth itself is the §19 item 19 question the owner still
   owns.

## Not defects

- **`inbox.list` takes `scope` as a free-form string.** A value outside
  `inbox|unread|sent|archived` is a usage error at exit 2 rather than a
  schema rejection. The CLI's `--scope` is the same shape, and the two must
  answer alike, so tightening the tool alone would make them disagree.
- **`--announcements` is a breaking change.** `feat(discussions)!` marks it,
  §19 item 27 authorizes it, and the README row follows.
