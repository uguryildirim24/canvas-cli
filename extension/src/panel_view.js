// What the panel says about what the host sent it.
//
// This file holds every word the panel puts next to a state name, and it is
// pure so those words can be tested. One rule governs all of them:
//
//   **Nothing is shown as done that the journal does not say is done.**
//
// SPEC §12.2 names the journal states, and the panel prints those names
// verbatim. The sentence beside a name explains it; it never replaces it and
// never rounds it up. `matched` says the attribution is unproven, because
// Canvas showed the attempt and this machine did not observe the request that
// created it. `outcome_unknown` says the outcome was never observed. Neither
// is a submission, and the panel never draws them as one.
globalThis.canvasCli = globalThis.canvasCli || {};

// How a journal state reads to a person, and whether it is finished work.
//
// `done` is true only for `submitted`: the one state in which this machine
// observed the response that created the attempt.
const JOURNAL_STATES = {
  planned: { done: false, running: true, says: "frozen, nothing has left this machine yet" },
  uploading: { done: false, running: true, says: "uploading files" },
  uploaded: { done: false, running: true, says: "files are uploaded, nothing is posted" },
  posting: { done: false, running: true, says: "the submission was sent; no answer yet" },
  submitted: { done: true, running: false, says: "submitted, and this machine saw the answer" },
  matched: {
    done: false,
    running: false,
    says: "attribution unproven: Canvas shows an attempt with exactly these files, and this machine did not see the request that made it",
  },
  upload_incomplete: { done: false, running: false, says: "an upload failed; nothing was posted" },
  uploaded_not_submitted: {
    done: false,
    running: false,
    says: "every file is uploaded and the submission was never sent",
  },
  outcome_unknown: {
    done: false,
    running: false,
    says: "outcome unknown: the submission was sent and its outcome was never observed",
  },
  refused: { done: false, running: false, says: "refused; nothing was submitted" },
};

/**
 * One journal row as the panel shows it.
 *
 * `state` comes straight through. `says` explains it. `done` is what the
 * panel is allowed to draw as finished, and it is false for every state but
 * `submitted`.
 */
globalThis.canvasCli.journalView = function journalView(journal) {
  const state = String(journal && journal.state ? journal.state : "");
  const known = JOURNAL_STATES[state] || {
    done: false,
    running: false,
    says: "this state is not one this panel knows; read it in the CLI",
  };
  const notes = [];
  if (journal && journal.superseded) {
    notes.push("superseded by a later attempt");
  }
  if (journal && journal.acknowledged) {
    notes.push("the unknown outcome was accepted by you");
  }
  return {
    journal_id: String((journal && journal.journal_id) || ""),
    state,
    says: known.says,
    done: known.done,
    running: known.running,
    kind: String((journal && journal.kind) || ""),
    // An operation need not name an assignment at all, so the kind is the
    // fallback rather than an invented "assignment ?".
    assignment: label(journal),
    updated_at: String((journal && journal.updated_at) || ""),
    // A receipt exists, or it does not. "observed" and "unknown" are
    // different rows and the panel never merges them.
    receipt: journal && journal.receipt_id ? String(journal.receipt_id) : null,
    notes,
  };
};

/** What to call one journal row on screen. */
function label(journal) {
  if (journal && journal.assignment_name) {
    return String(journal.assignment_name);
  }
  if (journal && journal.assignment_id) {
    return `assignment ${journal.assignment_id}`;
  }
  if (journal && journal.course_id) {
    return `course ${journal.course_id}`;
  }
  return String((journal && journal.kind) || "this operation");
}

/** How a follow reads: the acknowledgement first, the load outcome after. */
globalThis.canvasCli.followView = function followView(follow) {
  if (!follow) {
    return null;
  }
  const load = String(follow.load || "unknown");
  const says =
    load === "loaded"
      ? "the page loaded"
      : load === "failed"
        ? "the page did not load"
        : "the browser took the request; how it ended is not known yet";
  return {
    url: String(follow.url || ""),
    dispatched: Boolean(follow.dispatched),
    dispatch_ms: typeof follow.dispatch_ms === "number" ? follow.dispatch_ms : null,
    load,
    says,
    // Navigating is not a read of the API. A Canvas page can change state
    // when it is opened; a discussion marks itself read (REPORT §3.2, S13).
    side_effects: "opening a Canvas page can change it: a discussion marks itself read",
  };
};

/**
 * The API facts for the page, as the host read them from the local cache.
 *
 * They are whole §7 envelopes and they stay whole: each one keeps its own
 * freshness, and a browser observation never updates one of them (REPORT
 * §3.1). What the panel takes out is a name, a due date, and how old the row
 * is — never page text, which is not in these envelopes at all.
 */
globalThis.canvasCli.apiView = function apiView(api) {
  const rows = [];
  const add = (kind, envelope, title) => {
    if (!envelope) {
      return;
    }
    rows.push({
      kind,
      title,
      // A refused or unavailable envelope says so instead of a name.
      outcome: String(envelope.outcome || "ok"),
      freshness: (Array.isArray(envelope.freshness) ? envelope.freshness : []).map((row) => ({
        dataset: String(row.dataset || ""),
        state: String(row.state || ""),
        fetched_at: row.fetched_at ? String(row.fetched_at) : null,
      })),
    });
  };
  const course = api && api.course;
  const assignment = api && api.assignment;
  const announcement = api && api.announcement;
  add("course", course, name(course, "course", ["name", "code"]));
  add("assignment", assignment, name(assignment, "assignment", ["name", "title"]));
  add("announcement", announcement, name(announcement, "announcement", ["title", "name"]));
  return rows;
};

/** The first of `fields` the envelope's named result carries. */
function name(envelope, key, fields) {
  const result = envelope && envelope.result && envelope.result[key];
  if (!result) {
    return null;
  }
  for (const field of fields) {
    if (typeof result[field] === "string" && result[field] !== "") {
      return result[field];
    }
  }
  return null;
}

/** What the header says about the attachment. */
globalThis.canvasCli.attachmentView = function attachmentView(state) {
  const name = String((state && state.attachment_state) || "not_attached");
  const says = {
    attached: "this tab is shared with canvas-cli",
    validating: "checking that the browser and the CLI are the same Canvas account",
    paused: "sharing is paused; nothing is being read",
    not_attached: "nothing is shared; press the toolbar button to share this tab",
  }[name];
  return {
    state: name,
    says: says || "this state is not one this panel knows",
    consumers: Array.isArray(state && state.consumers) ? state.consumers.slice() : [],
    origin: String((state && state.origin) || ""),
    zone: state && state.zone ? String(state.zone) : null,
    title: state && state.title ? String(state.title) : null,
    url: state && state.url ? String(state.url) : null,
    observed_at: state && state.observed_at ? String(state.observed_at) : null,
    // The panel's own feed can fall behind the log. When it does, the panel
    // says "refresh" rather than showing a list it cannot vouch for.
    refresh_required: Boolean(state && state.resync_required),
  };
};

/** The whole panel, as plain data. */
globalThis.canvasCli.panelView = function panelView(state) {
  const attachment = globalThis.canvasCli.attachmentView(state);
  const origin = attachment.origin;
  const journals = Array.isArray(state && state.journals) ? state.journals : [];
  const notes = Array.isArray(state && state.notes) ? state.notes : [];
  return {
    attachment,
    api: globalThis.canvasCli.apiView(state && state.api),
    follow: globalThis.canvasCli.followView(state && state.follow),
    journals: journals.map((journal) => globalThis.canvasCli.journalView(journal)),
    approvals: Array.isArray(state && state.approvals) ? state.approvals.slice() : [],
    notes: notes.map((note) => ({
      note_id: String(note.note_id || ""),
      consumer: note.consumer ? String(note.consumer) : null,
      at: String(note.at || ""),
      generation: typeof note.generation === "number" ? note.generation : null,
      blocks: globalThis.canvasCli.renderMarkdown(note.text, origin),
      source_refs: (Array.isArray(note.source_refs) ? note.source_refs : []).map((ref) => ({
        ref: String(ref),
        policy: globalThis.canvasCli.linkPolicy(ref, origin),
      })),
    })),
  };
};
