// What may leave the browser, and how much of it.
//
// The host applies all of this again. Doing it here as well means the bytes
// never exist outside Chrome in the first place.
globalThis.canvasCli = globalThis.canvasCli || {};

// Query parameters that can carry a capability and never a route.
// `x-amz-` is a prefix: the signed-URL family spells several of them.
const CAPABILITY_PARAMS = new Set([
  "verifier",
  "signature",
  "token",
  "access_token",
  "sig",
  "policy",
  "expires",
  "session_token",
]);

/** Whether a query parameter name carries a capability. */
globalThis.canvasCli.isCapabilityParam = function isCapabilityParam(name) {
  const lower = String(name || "").toLowerCase();
  return lower.startsWith("x-amz-") || CAPABILITY_PARAMS.has(lower);
};

/**
 * A URL with every capability-bearing parameter, credential, and fragment
 * removed. A URL this code cannot parse becomes `null`.
 */
globalThis.canvasCli.sanitizeUrl = function sanitizeUrl(raw) {
  let url;
  try {
    url = new URL(String(raw));
  } catch {
    return null;
  }
  if (url.protocol !== "https:" && url.protocol !== "http:") {
    return null;
  }
  for (const name of [...url.searchParams.keys()]) {
    if (globalThis.canvasCli.isCapabilityParam(name)) {
      url.searchParams.delete(name);
    }
  }
  url.hash = "";
  url.username = "";
  url.password = "";
  return url.toString();
};

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: false });

/** The UTF-8 length of a string. */
globalThis.canvasCli.utf8Length = function utf8Length(text) {
  return encoder.encode(String(text || "")).length;
};

/**
 * Cut `text` to at most `max` UTF-8 bytes, on a character boundary.
 * @returns {{text: string, truncated: boolean}}
 */
globalThis.canvasCli.truncateUtf8 = function truncateUtf8(text, max) {
  const value = String(text || "");
  const bytes = encoder.encode(value);
  if (bytes.length <= max) {
    return { text: value, truncated: false };
  }
  let end = max;
  // A UTF-8 continuation byte is 10xxxxxx; step back off one.
  while (end > 0 && (bytes[end] & 0xc0) === 0x80) {
    end -= 1;
  }
  return { text: decoder.decode(bytes.subarray(0, end)), truncated: true };
};

/**
 * Apply the total payload ceiling across the selection and the excerpt.
 *
 * The selection is what the person pointed at, so it is kept first.
 */
globalThis.canvasCli.boundExtract = function boundExtract(extract) {
  const out = {
    selection: extract.selection ?? null,
    text: extract.text ?? null,
    truncated: Boolean(extract.truncated),
  };
  let budget = globalThis.canvasCli.MAX_PAYLOAD_BYTES;
  if (out.selection !== null) {
    const cut = globalThis.canvasCli.truncateUtf8(out.selection, budget);
    out.selection = cut.text;
    out.truncated = out.truncated || cut.truncated;
    budget -= globalThis.canvasCli.utf8Length(out.selection);
  }
  if (out.text !== null) {
    const cut = globalThis.canvasCli.truncateUtf8(out.text, budget);
    out.text = cut.text;
    out.truncated = out.truncated || cut.truncated;
  }
  return out;
};

// Field names that look like a credential. A field whose name matches is
// never read, whatever its type says.
const CREDENTIAL_NAMES = [
  "password",
  "passwd",
  "token",
  "secret",
  "authenticity",
  "csrf",
  "api_key",
  "apikey",
  "session",
  "auth",
  "otp",
  "pin",
  "credit",
  "card",
  "cvv",
  "ssn",
];

/** Whether a form field's name or id looks like a credential. */
globalThis.canvasCli.isCredentialField = function isCredentialField(name) {
  const lower = String(name || "").toLowerCase();
  return CREDENTIAL_NAMES.some((needle) => lower.includes(needle));
};

/** Collapse runs of whitespace, the way a reader sees the text. */
globalThis.canvasCli.normalizeText = function normalizeText(text) {
  return String(text || "")
    .replace(/[ \t\r\f\v]+/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .split("\n")
    .map((line) => line.trim())
    .join("\n")
    .trim();
};
