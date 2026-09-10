import { test } from "node:test";
import assert from "node:assert/strict";

import { companion } from "./load.js";

const {
  MAX_PAYLOAD_BYTES,
  boundExtract,
  isCapabilityParam,
  isCredentialField,
  normalizeText,
  sanitizeUrl,
  truncateUtf8,
  utf8Length,
} = companion;

test("capability parameters are named case-insensitively", () => {
  for (const name of [
    "verifier",
    "Verifier",
    "Signature",
    "X-Amz-Signature",
    "x-amz-credential",
    "token",
    "sig",
    "Policy",
    "Expires",
  ]) {
    assert.ok(isCapabilityParam(name), name);
  }
  for (const name of ["page", "per_page", "module_item_id", "anchor"]) {
    assert.equal(isCapabilityParam(name), false, name);
  }
});

test("a sanitized URL keeps the route and drops the capability", () => {
  const url = sanitizeUrl(
    "https://school.test/courses/1/files/9/download?verifier=SECRET&wrap=1&X-Amz-Signature=deadbeef#frag"
  );
  assert.ok(url.includes("wrap=1"), url);
  assert.ok(!url.includes("SECRET"), url);
  assert.ok(!url.toLowerCase().includes("amz"), url);
  assert.ok(!url.includes("#"), url);
});

test("credentials in a URL are never shared", () => {
  const url = sanitizeUrl("https://user:pass@school.test/courses/1");
  assert.ok(!url.includes("user"), url);
  assert.ok(!url.includes("pass"), url);
});

test("a URL this code cannot inspect is dropped", () => {
  for (const raw of ["javascript:alert(1)", "not a url", "data:text/html,<b>x</b>", ""]) {
    assert.equal(sanitizeUrl(raw), null, raw);
  }
});

test("the byte bound never splits a character", () => {
  const text = "\u{1F600}".repeat(MAX_PAYLOAD_BYTES);
  const cut = truncateUtf8(text, MAX_PAYLOAD_BYTES);
  assert.ok(cut.truncated);
  assert.ok(utf8Length(cut.text) <= MAX_PAYLOAD_BYTES);
  // Every kept code point survived whole.
  assert.ok([...cut.text].every((c) => c === "\u{1F600}"), "a character was split");
  assert.equal(utf8Length(cut.text) % 4, 0);
});

test("a two-byte character is not split either", () => {
  const cut = truncateUtf8("é".repeat(MAX_PAYLOAD_BYTES), MAX_PAYLOAD_BYTES);
  assert.ok(cut.truncated);
  assert.ok([...cut.text].every((c) => c === "é"));
});

test("the total payload is bounded across both fields", () => {
  const bounded = boundExtract({
    selection: "é".repeat(MAX_PAYLOAD_BYTES),
    text: "x".repeat(MAX_PAYLOAD_BYTES),
    truncated: false,
  });
  assert.ok(bounded.truncated);
  assert.ok(
    utf8Length(bounded.selection) + utf8Length(bounded.text) <= MAX_PAYLOAD_BYTES
  );
});

test("a payload inside the bound is untouched and unmarked", () => {
  const bounded = boundExtract({
    selection: "a short passage",
    text: "the visible excerpt",
    truncated: false,
  });
  assert.equal(bounded.truncated, false);
  assert.equal(bounded.selection, "a short passage");
  assert.equal(bounded.text, "the visible excerpt");
});

test("credential-looking field names are named", () => {
  for (const name of [
    "authenticity_token",
    "user[password]",
    "csrf_token",
    "api_key",
    "session_id",
    "SECRET_value",
    "cvv",
  ]) {
    assert.ok(isCredentialField(name), name);
  }
  for (const name of ["submission[body]", "comment", "essay_text"]) {
    assert.equal(isCredentialField(name), false, name);
  }
});

test("normalized text keeps the reader's line structure", () => {
  assert.equal(normalizeText("  a   b \n\n\n c  "), "a b\n\nc");
});
