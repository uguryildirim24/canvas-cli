// Reading the page, on request only.
//
// Nothing here runs until the zone is `open` or `graded` and a consumer has
// asked for text. It reads an allowlist of Canvas content containers, the
// person's own selection, and the visible editor excerpt. It never reads a
// hidden input, a field whose name looks like a credential, or a frame.
globalThis.canvasCli = globalThis.canvasCli || {};

// The Canvas containers that hold coursework prose. Anything not named here
// is not read, so a Canvas page this release does not know contributes
// nothing rather than everything.
const CONTENT_SELECTORS = [
  ".show-content",
  ".user_content",
  ".description",
  ".discussion-section .message",
  ".message_wrapper",
  ".assignment-description",
  "#assignment_show .description",
  "#wiki_page_show .show-content",
];

// The editor a person may be typing into. `iframe` editors are deliberately
// absent: the companion never reaches into a frame.
const EDITOR_SELECTORS = [
  "textarea#submission_body",
  "textarea.submission_text_entry",
  ".ProseMirror",
  "[contenteditable='true']",
];

/** The heading a person would call the page. */
const TITLE_SELECTORS = ["h1.title", ".assignment-title", "h1", "title"];

/**
 * Every frame on the document, as the zone classifier wants them.
 *
 * This is the only thing read before classification, and it reads attributes,
 * never contents.
 */
globalThis.canvasCli.collectFrames = function collectFrames(root) {
  const frames = [];
  for (const frame of root.querySelectorAll("iframe")) {
    frames.push({
      id: attr(frame, "id"),
      name: attr(frame, "name"),
      src: attr(frame, "src"),
      title: attr(frame, "title"),
      className: attr(frame, "class"),
    });
  }
  return frames;
};

/** The page title, bounded. */
globalThis.canvasCli.pageTitle = function pageTitle(root) {
  for (const selector of TITLE_SELECTORS) {
    const found = root.querySelectorAll(selector)[0];
    if (found) {
      const text = globalThis.canvasCli.normalizeText(found.textContent);
      if (text.length > 0) {
        return globalThis.canvasCli.truncateUtf8(text, 512).text;
      }
    }
  }
  return null;
};

/**
 * The allowlisted text of one document.
 *
 * @param {object} root a document or element with `querySelectorAll`
 * @param {object} options `{ zone, selection }`
 * @returns {{selection: string|null, text: string|null, truncated: boolean}}
 */
globalThis.canvasCli.extractText = function extractText(root, options) {
  const zone = (options && options.zone) || "unknown";
  // Classification comes first, always. An opaque zone returns nothing, and
  // no selector below has run by the time this returns.
  if (globalThis.canvasCli.isOpaque(zone)) {
    return { selection: null, text: null, truncated: false };
  }
  const pieces = [];
  for (const selector of CONTENT_SELECTORS) {
    for (const element of root.querySelectorAll(selector)) {
      if (isSuppressed(element)) {
        continue;
      }
      const text = globalThis.canvasCli.normalizeText(element.textContent);
      if (text.length > 0 && !pieces.includes(text)) {
        pieces.push(text);
      }
    }
  }
  for (const selector of EDITOR_SELECTORS) {
    for (const element of root.querySelectorAll(selector)) {
      if (isSuppressed(element) || isCredentialElement(element)) {
        continue;
      }
      const raw = typeof element.value === "string" ? element.value : element.textContent;
      const text = globalThis.canvasCli.normalizeText(raw);
      if (text.length > 0 && !pieces.includes(text)) {
        pieces.push(text);
      }
    }
  }
  const selection = globalThis.canvasCli.normalizeText(
    (options && options.selection) || ""
  );
  return globalThis.canvasCli.boundExtract({
    selection: selection.length > 0 ? selection : null,
    text: pieces.length > 0 ? pieces.join("\n\n") : null,
    truncated: false,
  });
};

/** Whether an element is hidden from the reader, and so from the companion. */
function isSuppressed(element) {
  const tag = String(element.tagName || "").toLowerCase();
  if (tag === "script" || tag === "style" || tag === "noscript" || tag === "iframe") {
    return true;
  }
  if (tag === "input") {
    const type = String(attr(element, "type") || "").toLowerCase();
    // A hidden input is never read, whatever it holds.
    if (type === "hidden" || type === "password") {
      return true;
    }
  }
  if (attr(element, "hidden") !== null) {
    return true;
  }
  if (attr(element, "aria-hidden") === "true") {
    return true;
  }
  const style = String(attr(element, "style") || "").replace(/\s+/g, "").toLowerCase();
  if (style.includes("display:none") || style.includes("visibility:hidden")) {
    return true;
  }
  return isCredentialElement(element);
}

/** Whether an element's own name or id looks like a credential. */
function isCredentialElement(element) {
  return (
    globalThis.canvasCli.isCredentialField(attr(element, "name")) ||
    globalThis.canvasCli.isCredentialField(attr(element, "id")) ||
    globalThis.canvasCli.isCredentialField(attr(element, "autocomplete"))
  );
}

function attr(element, name) {
  if (typeof element.getAttribute === "function") {
    const value = element.getAttribute(name);
    return value === undefined ? null : value;
  }
  return null;
}
