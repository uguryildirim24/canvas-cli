import { test } from "node:test";
import assert from "node:assert/strict";

import { companion } from "./load.js";

const { classifyRoute } = companion;

test("the routes the companion supports classify", () => {
  const assignment = classifyRoute("/courses/45679/assignments/98765");
  assert.equal(assignment.kind, "assignment");
  assert.equal(assignment.course_id, "45679");
  assert.equal(assignment.assignment_id, "98765");

  const quiz = classifyRoute("/courses/1/quizzes/321/take");
  assert.equal(quiz.kind, "quiz");
  assert.equal(quiz.quiz_id, "321");

  const topic = classifyRoute("/courses/1/discussion_topics/7");
  assert.equal(topic.kind, "discussion");
  assert.equal(topic.topic_id, "7");

  const page = classifyRoute("/courses/1/pages/week-one");
  assert.equal(page.kind, "page");
  assert.equal(page.page_url, "week-one");

  assert.equal(classifyRoute("/").kind, "dashboard");
  assert.equal(classifyRoute("/dashboard").kind, "dashboard");
  assert.equal(classifyRoute("/calendar").kind, "calendar");
  assert.equal(classifyRoute("/courses/1").kind, "course");
  assert.equal(classifyRoute("/courses/1/assignments").kind, "course");
  assert.equal(classifyRoute("/courses/1/modules").kind, "modules");
  assert.equal(classifyRoute("/courses/1/grades").kind, "grades");
  assert.equal(classifyRoute("/files/9").kind, "files");
});

test("an id that is not a number is not an id", () => {
  for (const path of [
    "/courses/not-a-number/assignments/7",
    "/courses/1/assignments/../../etc/passwd",
    "/courses/1e9/assignments/2",
  ]) {
    const route = classifyRoute(path);
    assert.equal(route.assignment_id, null, path);
  }
  // A course id that is not numeric yields no course either.
  assert.equal(classifyRoute("/courses/abc").course_id, null);
});

test("an unrecognized path names nothing", () => {
  for (const path of ["/lti/launch", "/some/tool", "/login/canvas", ""]) {
    const route = classifyRoute(path);
    assert.equal(route.course_id, null, path);
    assert.equal(route.assignment_id, null, path);
  }
});

test("the route classifier matches the host's", async () => {
  // The two implementations are the same table; this pins the pairs the Rust
  // test also pins, so a change on one side fails on both.
  const pairs = [
    ["/courses/45679/assignments/98765", "assignment"],
    ["/courses/45679/quizzes/321/take", "quiz"],
    ["/courses/1/discussion_topics/7", "discussion"],
    ["/courses/1/pages/week-one", "page"],
    ["/", "dashboard"],
    ["/some/unknown/tool", "other"],
  ];
  for (const [path, kind] of pairs) {
    assert.equal(classifyRoute(path).kind, kind, path);
  }
});
