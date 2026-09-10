// The isolated-world content script.
//
// Chrome injects this only after a gesture, and only into the tab that
// gesture named (`activeTab`). It does three things, in this order:
//
// 1. classify the zone from the route and the frames on the page;
// 2. perform the one fixed same-origin account probe, in extension-owned
//    isolated code, and reduce its body to `{ id }` before anything leaves;
// 3. read the allowlisted text, and only when a consumer asked for it.
//
// The probe runs here, not in the service worker, because here it is
// same-origin: the browser's cookies answer it and never leave Chrome, and
// the extension needs no host permission of any kind.
(() => {
  const api = globalThis.canvasCli;

  // A fresh document is a fresh isolated world, so a value minted here is
  // exactly one document's identity.
  if (!api.documentId) {
    api.documentId = crypto.randomUUID();
  }
  if (api.installed) {
    return;
  }
  api.installed = true;

  let hiddenTimer = null;
  let pauseHiddenAfterMs = 10 * 60 * 1000;

  /** The one Canvas request this companion ever makes. */
  async function probeAccount() {
    try {
      const response = await fetch(location.origin + api.PROBE_PATH, {
        method: "GET",
        // A redirect is a login wall or another origin. Never follow one.
        redirect: "error",
        credentials: "same-origin",
        cache: "no-store",
        headers: { Accept: "application/json" },
      });
      if (!response.ok || response.redirected) {
        return null;
      }
      const body = await response.json();
      if (body === null || typeof body !== "object") {
        return null;
      }
      const id = body.id;
      if (typeof id !== "number" && typeof id !== "string") {
        return null;
      }
      // Reduced to `{ id }` here: no name, no email, no login id, no avatar.
      return { user_id: String(id), observed_at: new Date().toISOString() };
    } catch {
      // A refused redirect, a network failure, or a body that is not JSON.
      return null;
    }
  }

  /** One observation of this document, with nothing an opaque zone hides. */
  async function observe() {
    // Frames are read as attributes, before any text exists.
    const frames = api.collectFrames(document);
    // The address is captured with the frames, so what is classified and
    // what is reported are the same page. A same-document navigation during
    // the probe below would otherwise pair one page's zone with another
    // page's URL.
    const href = location.href;
    const { route, zone } = api.classifyZone(pathOf(href), frames);
    const account = await probeAccount();
    // The page moved while the probe was in flight: this classification
    // describes a document that is gone, so it describes nothing.
    const opaque = api.isOpaque(zone) || location.href !== href;
    return {
      origin: location.origin,
      tab_id: -1,
      document_id: api.documentId,
      frame_id: 0,
      navigation_generation: 0,
      route: opaque ? emptyRoute() : route,
      zone: location.href === href ? zone : "unknown",
      account,
      url: opaque ? null : api.sanitizeUrl(href),
      title: opaque ? null : api.pageTitle(document),
      observed_at: new Date().toISOString(),
    };
  }

  /** The path of an address, for the classifier. */
  function pathOf(href) {
    try {
      return new URL(href).pathname;
    } catch {
      return "";
    }
  }

  function emptyRoute() {
    return {
      kind: null,
      course_id: null,
      assignment_id: null,
      topic_id: null,
      quiz_id: null,
      page_url: null,
    };
  }

  /** Answer a text request: re-probe, then read. */
  async function extract() {
    const frames = api.collectFrames(document);
    const href = location.href;
    const { zone } = api.classifyZone(pathOf(href), frames);
    const account = await probeAccount();
    // The classification must still describe the page about to be read. A
    // document that moved under the probe is answered as `unknown`, which
    // reads nothing.
    if (location.href !== href) {
      return { zone: "unknown", account, extract: emptyExtract() };
    }
    if (account === null || api.isOpaque(zone)) {
      return { zone, account, extract: emptyExtract() };
    }
    const selection = String(globalThis.getSelection?.() ?? "");
    return {
      zone,
      account,
      extract: api.extractText(document, { zone, selection }),
    };
  }

  function emptyExtract() {
    return { selection: null, text: null, truncated: false };
  }

  function armHiddenTimer() {
    clearTimeout(hiddenTimer);
    if (document.visibilityState !== "hidden") {
      return;
    }
    hiddenTimer = setTimeout(() => {
      chrome.runtime.sendMessage({ type: "pause", cause: "hidden" }).catch(() => {});
    }, pauseHiddenAfterMs);
  }

  document.addEventListener("visibilitychange", armHiddenTimer);

  chrome.runtime.onMessage.addListener((message, _sender, respond) => {
    if (message.type === "observe") {
      pauseHiddenAfterMs = message.pause_hidden_after_ms || pauseHiddenAfterMs;
      observe().then(respond);
      return true;
    }
    if (message.type === "extract") {
      extract().then((answer) =>
        respond({
          document_id: api.documentId,
          ...answer,
        })
      );
      return true;
    }
    return false;
  });
})();
