import { test } from "node:test";
import assert from "node:assert/strict";

import { companion, fixture } from "./load.js";
import { parse } from "./dom.js";

const { classifyZone, collectFrames, extractText, utf8Length, MAX_PAYLOAD_BYTES } = companion;

/** Classify a fixture the way the content script does, then read it. */
function read(name, path, options = {}) {
  const root = parse(fixture(name));
  const { zone } = classifyZone(path, collectFrames(root));
  return { zone, extract: extractText(root, { zone, ...options }) };
}

test("an assignment page yields its prompt and the visible draft", () => {
  const { zone, extract } = read("assignment.html", "/courses/1/assignments/2");
  assert.equal(zone, "open");
  assert.ok(extract.text.includes("Write 1200 words"), extract.text);
  assert.ok(extract.text.includes("Cite three primary sources"), extract.text);
  assert.ok(extract.text.includes("My draft argues"), extract.text);
  assert.equal(extract.truncated, false);
});

test("no secret, token, or hidden field reaches the extract", () => {
  const { extract } = read("assignment.html", "/courses/1/assignments/2");
  const wire = JSON.stringify(extract);
  for (const secret of [
    "SECRET-CSRF-abcdef123456",
    "authenticity_token",
    "hunter2",
    "SECRET-ENV-token",
    "csrf_token",
    "eula_agreement_timestamp",
  ]) {
    assert.ok(!wire.includes(secret), `${secret} reached the wire: ${wire}`);
  }
});

test("hidden and aria-hidden content is not the page a reader sees", () => {
  const { extract } = read("assignment.html", "/courses/1/assignments/2");
  assert.ok(!extract.text.includes("staff-only"), extract.text);
  assert.ok(!extract.text.includes("Screen-reader scaffolding"), extract.text);
});

test("a quiz yields nothing at all", () => {
  const { zone, extract } = read("quiz.html", "/courses/1/quizzes/5/take", {
    selection: "Diocletian",
  });
  assert.equal(zone, "assessment");
  assert.equal(extract.text, null);
  assert.equal(extract.selection, null);
  assert.equal(extract.truncated, false);
});

test("an external tool page yields nothing, and no launch token", () => {
  const { zone, extract } = read("external_tool.html", "/courses/1/assignments/2");
  assert.equal(zone, "external");
  assert.equal(extract.text, null);
  assert.ok(!JSON.stringify(extract).includes("SECRET-LTI"));
});

test("an unrecognized frame makes the whole page opaque", () => {
  const { zone, extract } = read("unknown_frame.html", "/courses/1/pages/week-one");
  assert.equal(zone, "unknown");
  assert.equal(extract.text, null);
});

test("a login page yields no credential, whatever its route says", () => {
  const root = parse(fixture("login.html"));
  // Even asked to read it as an open course page, the fields are refused.
  const extract = extractText(root, { zone: "open", selection: "" });
  const wire = JSON.stringify(extract);
  assert.ok(!wire.includes("hunter2"), wire);
  assert.ok(!wire.includes("SECRET-CSRF"), wire);
  assert.ok(!wire.includes("sam@school.test"), wire);
});

test("a discussion yields the posts a reader sees", () => {
  const { zone, extract } = read("discussion.html", "/courses/1/discussion_topics/7");
  assert.equal(zone, "open");
  assert.ok(extract.text.includes("Post your reading response"), extract.text);
  assert.ok(extract.text.includes("Reply to two classmates"), extract.text);
});

test("the selection is carried and bounded with the excerpt", () => {
  const { extract } = read("assignment.html", "/courses/1/assignments/2", {
    selection: "  causes of the fall  ",
  });
  assert.equal(extract.selection, "causes of the fall");
  assert.ok(
    utf8Length(extract.selection) + utf8Length(extract.text) <= MAX_PAYLOAD_BYTES
  );
});

test("the title is the heading a reader would name", () => {
  const root = parse(fixture("assignment.html"));
  assert.equal(companion.pageTitle(root), "Essay 1: The Fall of Rome");
});

test("classification precedes extraction: an opaque zone runs no selector", () => {
  const root = parse(fixture("assignment.html"));
  let selectorsRun = 0;
  const watched = {
    querySelectorAll(selector) {
      selectorsRun += 1;
      return root.querySelectorAll(selector);
    },
  };
  const extract = extractText(watched, { zone: "assessment", selection: "anything" });
  assert.equal(selectorsRun, 0, "an opaque zone read the document");
  assert.equal(extract.text, null);
  assert.equal(extract.selection, null);
});
