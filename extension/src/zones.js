// Zone classification, which happens BEFORE any text is read.
//
// The five zones are REPORT section 3.5's: `open`, `graded`, `assessment`,
// `external`, `unknown`. The last three expose nothing at all — no title, no
// URL beyond the origin, no route ids, no text.
//
// Two things decide a zone: the route, and which frames the document carries.
// The stricter reading wins, always.
globalThis.canvasCli = globalThis.canvasCli || {};

// Most exposed first. `strictest` takes the later of two.
const ORDER = ["open", "graded", "assessment", "external", "unknown"];

const OPAQUE = new Set(["assessment", "external", "unknown"]);

/** Whether a zone exposes nothing. */
globalThis.canvasCli.isOpaque = function isOpaque(zone) {
  return OPAQUE.has(zone);
};

/** The stricter of two classifications. */
globalThis.canvasCli.strictest = function strictest(a, b) {
  const left = ORDER.indexOf(a);
  const right = ORDER.indexOf(b);
  if (left < 0 || right < 0) {
    return "unknown";
  }
  return ORDER[Math.max(left, right)];
};

/** The zone a route alone implies. */
globalThis.canvasCli.zoneForRoute = function zoneForRoute(route) {
  switch (route && route.kind) {
    case "quiz":
      return "assessment";
    case "grades":
      return "graded";
    case "dashboard":
    case "course":
    case "assignment":
    case "announcement":
    case "discussion":
    case "modules":
    case "files":
    case "page":
    case "calendar":
      return "open";
    default:
      return "unknown";
  }
};

// A frame Canvas itself owns for ordinary content. Anything else embedded in
// a Canvas page is an external tool or something this release cannot name.
const KNOWN_FRAME_IDS = new Set([
  "preview_frame",
  "wiki_page_show",
  "speed_grader_iframe",
]);

// Frames that mean an assessment is on the page.
const ASSESSMENT_HINTS = ["quiz", "assessment", "exam", "proctor", "lockdown"];

// Frames that mean an external tool is on the page.
const EXTERNAL_HINTS = ["tool_content", "lti", "external_tool", "basic_lti"];

/**
 * Classify one document from its path and its frames.
 *
 * `frames` is `[{ id, name, src, title, className }]`, collected before any
 * text is read.
 */
globalThis.canvasCli.classifyZone = function classifyZone(path, frames) {
  const route = globalThis.canvasCli.classifyRoute(path);
  let zone = globalThis.canvasCli.zoneForRoute(route);
  for (const frame of frames || []) {
    zone = globalThis.canvasCli.strictest(zone, zoneForFrame(frame));
  }
  return { route, zone };
};

function zoneForFrame(frame) {
  const haystack = [frame.id, frame.name, frame.src, frame.title, frame.className]
    .filter((value) => typeof value === "string")
    .join(" ")
    .toLowerCase();
  if (ASSESSMENT_HINTS.some((hint) => haystack.includes(hint))) {
    return "assessment";
  }
  if (EXTERNAL_HINTS.some((hint) => haystack.includes(hint))) {
    return "external";
  }
  if (typeof frame.id === "string" && KNOWN_FRAME_IDS.has(frame.id)) {
    return "open";
  }
  // An unknown frame is not searched for clues by first reading its contents
  // (REPORT section 3.3). It makes the page opaque instead.
  return "unknown";
}
