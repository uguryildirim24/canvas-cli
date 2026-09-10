import { test } from "node:test";
import assert from "node:assert/strict";

import { companion, fixture } from "./load.js";

const { apiView, attachmentView, followView, journalView, panelView } = companion;

const ORIGIN = "https://courses.example.test";

function journal(state, extra = {}) {
  return {
    journal_id: `j-${state}`,
    state,
    course_id: "45679",
    assignment_id: "98765",
    assignment_name: "Problem Set 3",
    kind: "online_upload",
    updated_at: "2026-09-10T16:04:40Z",
    receipt_id: null,
    superseded: false,
    acknowledged: false,
    ...extra,
  };
}

test("only a submitted journal is shown as done", () => {
  for (const state of [
    "planned",
    "uploading",
    "uploaded",
    "posting",
    "matched",
    "upload_incomplete",
    "uploaded_not_submitted",
    "outcome_unknown",
    "refused",
  ]) {
    const view = journalView(journal(state));
    assert.equal(view.done, false, state);
    // The SPEC §12.2 name reaches the person unchanged.
    assert.equal(view.state, state);
    assert.ok(view.says.length > 0, state);
  }
  assert.equal(journalView(journal("submitted")).done, true);
});

test("matched says the attribution is unproven, and unknown says it is unknown", () => {
  assert.ok(journalView(journal("matched")).says.includes("attribution unproven"));
  assert.ok(journalView(journal("outcome_unknown")).says.includes("outcome unknown"));
});

test("an observed receipt and an unknown outcome are different rows", () => {
  const observed = journalView(journal("submitted", { receipt_id: "r-1" }));
  const unknown = journalView(journal("outcome_unknown"));
  assert.equal(observed.receipt, "r-1");
  assert.equal(unknown.receipt, null);
  assert.notEqual(observed.says, unknown.says);
  assert.notEqual(observed.done, unknown.done);
});

test("a state this panel does not know is never called done", () => {
  const view = journalView(journal("something_new"));
  assert.equal(view.state, "something_new");
  assert.equal(view.done, false);
  assert.ok(view.says.includes("not one this panel knows"));
});

test("acknowledged and superseded are said, not folded into the state", () => {
  const view = journalView(journal("outcome_unknown", { acknowledged: true, superseded: true }));
  assert.equal(view.state, "outcome_unknown");
  assert.equal(view.done, false);
  assert.equal(view.notes.length, 2);
});

test("a row that names no assignment is not called one", () => {
  const operation = journalView({
    journal_id: "j-op",
    state: "outcome_unknown",
    course_id: null,
    assignment_id: null,
    assignment_name: null,
    kind: "inbox_send",
    updated_at: "2026-09-10T16:04:40Z",
    receipt_id: null,
    superseded: false,
    acknowledged: false,
  });
  assert.equal(operation.assignment, "inbox_send");
  assert.equal(operation.done, false);
});

test("a follow separates the acknowledgement from the load outcome", () => {
  const dispatched = followView({
    request_id: "r",
    url: `${ORIGIN}/courses/1`,
    dispatched: true,
    dispatched_at: "2026-09-10T16:04:39Z",
    dispatch_ms: 41,
    generation: 3,
    load: "unknown",
    load_at: null,
  });
  assert.equal(dispatched.dispatched, true);
  assert.equal(dispatched.load, "unknown");
  assert.ok(dispatched.says.includes("not known yet"));
  // Navigating is not a read: the panel says so every time.
  assert.ok(dispatched.side_effects.includes("marks itself read"));

  assert.ok(followView({ load: "loaded" }).says.includes("loaded"));
  assert.ok(followView({ load: "failed" }).says.includes("did not load"));
  assert.equal(followView(null), null);
});

test("the attachment header names the state the host sent", () => {
  for (const state of ["attached", "validating", "paused", "not_attached"]) {
    const view = attachmentView({ attachment_state: state, origin: ORIGIN, consumers: [] });
    assert.equal(view.state, state);
    assert.ok(view.says.length > 0, state);
  }
  assert.equal(attachmentView({ resync_required: true }).refresh_required, true);
  assert.equal(attachmentView(null).state, "not_attached");
});

test("the API side keeps each envelope's own freshness", () => {
  const rows = apiView({
    course: {
      outcome: "ok",
      freshness: [{ dataset: "courses", state: "stale", fetched_at: "2026-09-01T00:00:00Z" }],
      result: { course: { name: "Intro to Computing", code: "CS-101" } },
    },
    assignment: {
      outcome: "unavailable",
      freshness: [],
      result: { assignment: null },
    },
    announcement: null,
  });
  assert.equal(rows.length, 2);
  assert.deepEqual(rows[0], {
    kind: "course",
    title: "Intro to Computing",
    outcome: "ok",
    freshness: [{ dataset: "courses", state: "stale", fetched_at: "2026-09-01T00:00:00Z" }],
  });
  // An envelope that names nothing says so; it does not borrow a name from
  // the browser observation beside it.
  assert.equal(rows[1].title, null);
  assert.equal(rows[1].outcome, "unavailable");
  assert.deepEqual(apiView(null), []);
});

test("the whole panel renders notes through the sanitizer", () => {
  const state = {
    attachment_state: "attached",
    consumers: ["mcp:claude-code"],
    origin: ORIGIN,
    zone: "open",
    title: "Essay 1",
    url: `${ORIGIN}/courses/45679/assignments/98765`,
    observed_at: "2026-09-10T16:04:40Z",
    journals: [journal("matched")],
    approvals: [],
    api: { course: null, assignment: null, announcement: null },
    notes: [
      {
        note_id: "n1",
        consumer: "mcp:claude-code",
        text: fixture("note-hostile.md"),
        source_refs: [`${ORIGIN}/courses/45679`, "canvas://receipts/9f1c", "https://evil.test/x"],
        at: "2026-09-10T16:04:38Z",
        generation: 3,
      },
    ],
    follow: null,
    cursor: 12,
    resync_required: false,
  };
  const view = panelView(state);
  assert.equal(view.attachment.state, "attached");
  assert.equal(view.journals[0].done, false);
  assert.equal(view.notes.length, 1);
  assert.deepEqual(
    view.notes[0].source_refs.map((ref) => ref.policy),
    ["allow", "ref", "block"]
  );
  // Not one node of a hostile note is a link, and none of it is a control.
  const stack = [...view.notes[0].blocks];
  const kinds = new Set();
  while (stack.length > 0) {
    const node = stack.pop();
    kinds.add(node.type);
    stack.push(...(node.children || []), ...(node.items || []).flat());
  }
  assert.equal(kinds.has("link"), false);
  assert.equal(kinds.has("button"), false);
});
