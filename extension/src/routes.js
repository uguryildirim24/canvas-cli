// Route classification. The mirror of `canvas_core::bridge::wire`.
//
// Only the path takes part. A query string can carry a capability
// (`verifier`, `Signature`, ...) and never identifies a route, and the host
// re-derives this from the sanitized URL before it believes any of it.
globalThis.canvasCli = globalThis.canvasCli || {};

const numeric = (value) => typeof value === "string" && /^[0-9]+$/.test(value);

/**
 * Classify a Canvas URL path.
 * @param {string} path
 * @returns {{kind: string|null, course_id: string|null, assignment_id: string|null,
 *            topic_id: string|null, quiz_id: string|null, page_url: string|null}}
 */
globalThis.canvasCli.classifyRoute = function classifyRoute(path) {
  const route = {
    kind: null,
    course_id: null,
    assignment_id: null,
    topic_id: null,
    quiz_id: null,
    page_url: null,
  };
  const parts = String(path || "").split("/").filter((part) => part.length > 0);
  if (parts.length === 0 || (parts.length === 1 && parts[0] === "dashboard")) {
    route.kind = "dashboard";
    return route;
  }
  if (parts.length === 1 && parts[0] === "calendar") {
    route.kind = "calendar";
    return route;
  }
  if (parts[0] === "files") {
    route.kind = "files";
    return route;
  }
  if (parts[0] !== "courses" || !numeric(parts[1])) {
    route.kind = "other";
    return route;
  }
  route.course_id = parts[1];
  const rest = parts.slice(2);
  if (rest.length === 0 || (rest.length === 1 && rest[0] === "assignments")) {
    route.kind = "course";
    return route;
  }
  switch (rest[0]) {
    case "assignments":
      if (numeric(rest[1])) {
        route.assignment_id = rest[1];
        route.kind = "assignment";
      } else {
        route.kind = "other";
      }
      return route;
    case "quizzes":
      if (numeric(rest[1])) {
        route.quiz_id = rest[1];
      }
      route.kind = "quiz";
      return route;
    case "announcements":
      route.kind = rest.length === 1 ? "announcement" : "other";
      return route;
    case "discussion_topics":
      if (numeric(rest[1])) {
        route.topic_id = rest[1];
        route.kind = "discussion";
      } else {
        route.kind = "other";
      }
      return route;
    case "pages":
      if (typeof rest[1] === "string" && rest[1].length > 0) {
        route.page_url = rest[1];
        route.kind = "page";
      } else {
        route.kind = "other";
      }
      return route;
    case "modules":
      route.kind = "modules";
      return route;
    case "files":
      route.kind = "files";
      return route;
    case "grades":
      route.kind = "grades";
      return route;
    default:
      route.kind = "other";
      return route;
  }
};
