import { test } from "node:test";
import assert from "node:assert/strict";

import { companion, fixture } from "./load.js";
import { parse } from "./dom.js";

const { classifyZone, collectFrames, isOpaque, strictest } = companion;

const framesOf = (name) => collectFrames(parse(fixture(name)));

test("an ordinary assignment page is open", () => {
  const { zone } = classifyZone("/courses/1/assignments/2", framesOf("assignment.html"));
  assert.equal(zone, "open");
  assert.equal(isOpaque(zone), false);
});

test("a quiz route is an assessment whatever the page carries", () => {
  const { zone } = classifyZone("/courses/1/quizzes/5/take", framesOf("quiz.html"));
  assert.equal(zone, "assessment");
  assert.ok(isOpaque(zone));
});

test("an external tool frame makes an open route external", () => {
  const { zone } = classifyZone(
    "/courses/1/assignments/2",
    framesOf("external_tool.html")
  );
  assert.equal(zone, "external");
  assert.ok(isOpaque(zone));
});

test("an unrecognized frame makes the page unknown, unsearched", () => {
  const { zone } = classifyZone("/courses/1/pages/week-one", framesOf("unknown_frame.html"));
  assert.equal(zone, "unknown");
  assert.ok(isOpaque(zone));
});

test("a Canvas preview frame does not make a page opaque", () => {
  const { zone } = classifyZone("/courses/1/discussion_topics/7", framesOf("discussion.html"));
  assert.equal(zone, "open");
});

test("a gradebook route is graded, and graded is not opaque", () => {
  const { zone } = classifyZone("/courses/1/grades", []);
  assert.equal(zone, "graded");
  assert.equal(isOpaque(zone), false);
});

test("the stricter classification always wins", () => {
  assert.equal(strictest("open", "assessment"), "assessment");
  assert.equal(strictest("assessment", "open"), "assessment");
  assert.equal(strictest("unknown", "open"), "unknown");
  assert.equal(strictest("open", "graded"), "graded");
  assert.equal(strictest("open", "nonsense"), "unknown");
});

test("frames are read as attributes, never as contents", () => {
  const frames = framesOf("external_tool.html");
  assert.equal(frames.length, 1);
  assert.deepEqual(Object.keys(frames[0]).sort(), [
    "className",
    "id",
    "name",
    "src",
    "title",
  ]);
});
