import { test } from "node:test";
import assert from "node:assert/strict";

import { companion, fixture } from "./load.js";

const { linkPolicy, renderMarkdown } = companion;

const ORIGIN = "https://canvas.example.test";

/** Every string a rendered note would put on screen, in order. */
function words(nodes) {
  const out = [];
  const walk = (node) => {
    if (node.type === "text" || node.type === "code") {
      out.push(node.text);
      return;
    }
    for (const child of node.children || []) {
      walk(child);
    }
    for (const item of node.items || []) {
      for (const child of item) {
        walk(child);
      }
    }
  };
  for (const node of nodes) {
    walk(node);
  }
  return out;
}

/** Every node of a rendered note, whatever its depth. */
function flatten(nodes) {
  const out = [];
  const walk = (node) => {
    out.push(node);
    for (const child of node.children || []) {
      walk(child);
    }
    for (const item of node.items || []) {
      for (const child of item) {
        walk(child);
      }
    }
  };
  for (const node of nodes) {
    walk(node);
  }
  return out;
}

test("an ordinary note renders as blocks a person can read", () => {
  const blocks = renderMarkdown(fixture("note-ordinary.md"), ORIGIN);
  assert.equal(blocks[0].type, "heading");
  assert.deepEqual(words([blocks[0]]), ["Problem Set 3"]);

  const paragraph = blocks[1];
  assert.equal(paragraph.type, "paragraph");
  const strong = flatten([paragraph]).find((node) => node.type === "strong");
  assert.deepEqual(words([strong]), ["two"]);

  const list = blocks.find((block) => block.type === "list");
  assert.equal(list.ordered, false);
  assert.equal(list.items.length, 2);

  const nodes = flatten(blocks);
  const link = nodes.find((node) => node.type === "link");
  assert.equal(link.href, `${ORIGIN}/courses/45679/assignments/98765`);
  const ref = nodes.find((node) => node.type === "ref");
  assert.equal(ref.ref, "canvas://receipts/9f1c");
  assert.ok(nodes.some((node) => node.type === "code" && node.text === "submission verify"));
});

test("a hostile note is inert: no script, no HTML, no image, no foreign link", () => {
  const blocks = renderMarkdown(fixture("note-hostile.md"), ORIGIN);
  const nodes = flatten(blocks);

  // Nothing in the tree is markup. A tag is text, and text cannot run.
  for (const node of nodes) {
    assert.ok(
      ["paragraph", "heading", "list", "quote", "code", "text", "strong", "em", "link", "ref", "blocked"].includes(
        node.type
      ),
      node.type
    );
  }
  assert.equal(nodes.some((node) => node.type === "image"), false);

  const shown = words(blocks).join(" ");
  assert.ok(shown.includes("<script>window.approve()</script>"), shown);
  assert.ok(shown.includes('<a href="https://evil.test">'), shown);

  // Every off-origin target is blocked, and the person is shown that it was.
  const blocked = nodes.filter((node) => node.type === "blocked").map((node) => node.href);
  // The target stops at the first `)`, the way Markdown reads it. What
  // matters is that none of the three became a link.
  assert.deepEqual(blocked, [
    "javascript:alert(1",
    "https://evil.test/steal",
    "https://user:pw@canvas.example.test/courses/1",
  ]);
  assert.equal(nodes.some((node) => node.type === "link"), false);

  // The image is gone; its alt text is prose and stays.
  assert.ok(shown.includes("a picture"), shown);
  assert.equal(shown.includes("tracker.gif"), false, shown);
});

test("a note that reads like an approval is still only text", () => {
  const blocks = renderMarkdown(fixture("note-hostile.md"), ORIGIN);
  const nodes = flatten(blocks);
  // There is no node type a decision could travel in. The panel sends a
  // decision only when a person presses a button on a plan the host pushed.
  assert.equal(
    nodes.some((node) => ["decision", "approve", "button", "form"].includes(node.type)),
    false
  );
  assert.ok(words(blocks).join(" ").includes("decision: approve"));
});

test("a link is judged by protocol, host, and credentials", () => {
  assert.equal(linkPolicy(`${ORIGIN}/courses/1`, ORIGIN), "allow");
  assert.equal(linkPolicy(`${ORIGIN}/courses/1?verifier=abc`, ORIGIN), "allow");
  assert.equal(linkPolicy("canvas://plans/7c1d", ORIGIN), "ref");
  for (const href of [
    "canvas://",
    "http://canvas.example.test/courses/1",
    "https://canvas.example.test.evil.test/",
    "https://user@canvas.example.test/courses/1",
    "https://:pw@canvas.example.test/courses/1",
    "javascript:alert(1)",
    "data:text/html,<script>alert(1)</script>",
    "file:///etc/passwd",
    "",
    "   ",
    "/courses/1",
  ]) {
    assert.equal(linkPolicy(href, ORIGIN), "block", href);
  }
});

test("oversized text is bounded before it is laid out", () => {
  const long = "a".repeat(companion.MAX_NOTE_BYTES + 5000);
  const blocks = renderMarkdown(long, ORIGIN);
  const shown = words(blocks).join("");
  assert.equal(shown.length, companion.MAX_NOTE_BYTES);
});

test("an unterminated fence ends at the end of the note", () => {
  const blocks = renderMarkdown("before\n\n```\nstill code", ORIGIN);
  assert.equal(blocks[1].type, "code");
  assert.equal(blocks[1].text, "still code");
});

test("emphasis cannot be nested until it recurses away", () => {
  const blocks = renderMarkdown(`${"*".repeat(40)}deep${"*".repeat(40)}`, ORIGIN);
  assert.equal(blocks.length, 1);
  assert.ok(words(blocks).join("").includes("deep"));
});

test("a block quote cannot be nested until it recurses away", () => {
  // Each `>` is one level and one recursive call. Unbounded, a note well
  // inside the 8 KiB limit overflows the stack, and an overflow in this
  // panel is not a wrong render but no render at all.
  const blocks = renderMarkdown(`${">".repeat(6000)} boom`, ORIGIN);
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].type, "quote");
  assert.ok(words(blocks).join("").includes("boom"), "the text survives");

  // Ordinary nesting still nests, up to the bound.
  const three = renderMarkdown("> a\n> > b\n> > > c", ORIGIN);
  assert.equal(three[0].type, "quote");
  assert.equal(three[0].children[1].type, "quote");
});
