// The service worker: the gesture, the native port, and the generations.
//
// It never reads a page. It owns four things:
//
// - the gesture. `activeTab` grants access to one tab when the person clicks
//   the toolbar action or presses the shortcut, and to nothing else;
// - the native-messaging port to `com.canvas_cli.bridge`;
// - the **navigation generation**, incremented on every committed navigation
//   of the attached tab. A cross-origin navigation ends the grant, so it ends
//   the attachment too and a new gesture is required;
// - the pauses: hidden tab, assessment, tab close, host loss;
// - the side panel: it opens on the same gesture, and it is fed only by what
//   the host pushes. The worker relays; it never composes what the panel
//   shows, and the panel's one message back — a decision on a plan — is
//   relayed to the host unchanged, for the host to check.
//
// It performs no fetch of its own. The one Canvas request lives in the
// content script, where it is same-origin.

const HOST_NAME = "com.canvas_cli.bridge";
const NATIVE_PROTOCOL = "bridge-native@1";

// The files injected into the isolated world, in dependency order. They are
// classic scripts sharing one global, because `scripting.executeScript` takes
// files rather than modules.
const INJECTED = [
  "src/shared.js",
  "src/routes.js",
  "src/zones.js",
  "src/sanitize.js",
  "src/extract.js",
  "src/content.js",
];

// One browser-profile instance, for the life of this service worker.
const PROFILE_INSTANCE = crypto.randomUUID();

// How long a navigation is watched before its outcome is called unknown.
const NAVIGATE_SETTLE_MS = 10 * 1000;

/** @type {{port: chrome.runtime.Port|null, tabId: number|null, origin: string|null,
 *          generation: number, pauseHiddenAfterMs: number, attached: boolean,
 *          paused: boolean, panel: object|null,
 *          navigation: {requestId: string, url: string, timer: number}|null}} */
const state = {
  port: null,
  tabId: null,
  origin: null,
  generation: 0,
  pauseHiddenAfterMs: 10 * 60 * 1000,
  attached: false,
  paused: false,
  // The last state the host pushed, so a panel opened later draws at once.
  panel: null,
  navigation: null,
};

chrome.action.onClicked.addListener((tab) => {
  attach(tab).catch(report);
});

chrome.commands.onCommand.addListener((command) => {
  if (command !== "attach") {
    return;
  }
  chrome.tabs
    .query({ active: true, currentWindow: true })
    .then(([tab]) => (tab ? attach(tab) : undefined))
    .catch(report);
});

chrome.tabs.onRemoved.addListener((tabId) => {
  if (tabId === state.tabId) {
    end("tab_closed");
  }
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
  if (tabId !== state.tabId || !state.attached) {
    return;
  }
  if (changeInfo.status !== "complete") {
    return;
  }
  // A committed navigation. Same origin keeps the `activeTab` grant; a
  // different origin revokes it, and with it the attachment.
  if (originOf(tab.url) !== state.origin) {
    end("cross_origin");
    return;
  }
  state.generation += 1;
  settleNavigation(tab.url);
  observe().catch(report);
});

chrome.runtime.onMessage.addListener((message) => {
  switch (message && message.type) {
    case "pause":
      state.paused = true;
      send({ type: "pause", cause: message.cause || "hidden" });
      return false;
    case "panel_hello":
      // Draw the panel from what is already here, then ask the host for the
      // current state. Nothing is composed in this worker.
      if (state.panel !== null) {
        toPanel({ type: "panel", state: state.panel });
      }
      send({ type: "panel_hello", protocol: NATIVE_PROTOCOL });
      return false;
    case "decision":
      // Straight through. This worker checks nothing and decides nothing;
      // the host owns the identity and re-checks every field.
      send({
        type: "decision",
        plan_id: String(message.plan_id || ""),
        handle: String(message.handle || ""),
        plan_sha256: String(message.plan_sha256 || ""),
        decision: String(message.decision || ""),
      });
      return false;
    default:
      return false;
  }
});

/** The gesture: connect the host, inject, and offer the attachment. */
async function attach(tab) {
  if (!tab || typeof tab.id !== "number") {
    return;
  }
  const origin = originOf(tab.url);
  if (origin === null) {
    return;
  }
  if (state.attached && state.tabId === tab.id) {
    // A second gesture on the same tab ends the attachment: the toolbar
    // button is the way to stop sharing as well as to start.
    end("user_detached");
    return;
  }
  state.tabId = tab.id;
  state.origin = origin;
  state.generation = 1;
  state.paused = false;
  // The panel opens on the same gesture that shares the tab, because opening
  // it needs that gesture and because the person should see what they just
  // shared. It is called before the first `await` for that reason.
  openPanel(tab.id);
  openPort();
  await inject(tab.id);
  const observation = await ask(tab.id, { type: "observe", pause_hidden_after_ms: state.pauseHiddenAfterMs });
  if (observation === null) {
    return;
  }
  state.attached = true;
  state.paused = false;
  send({
    type: "attach",
    observation: stamp(observation),
    consumer: null,
  });
}

/** Report a new document or a re-probe of the current one. */
async function observe() {
  if (state.tabId === null) {
    return;
  }
  await inject(state.tabId);
  const observation = await ask(state.tabId, {
    type: "observe",
    pause_hidden_after_ms: state.pauseHiddenAfterMs,
  });
  if (observation === null) {
    return;
  }
  send({ type: "update", observation: stamp(observation) });
  if (observation.zone === "assessment") {
    // Entering an assessment stops sharing, before anything is asked for.
    send({ type: "pause", cause: "assessment" });
  }
}

/** Put this worker's view of the tab on an observation. */
function stamp(observation) {
  return {
    ...observation,
    tab_id: state.tabId,
    navigation_generation: state.generation,
  };
}

/** Inject the isolated-world scripts into the granted tab. */
async function inject(tabId) {
  await chrome.scripting.executeScript({
    target: { tabId, allFrames: false },
    files: INJECTED,
  });
}

/** One request to the content script, or `null` when it cannot answer. */
async function ask(tabId, message) {
  try {
    const answer = await chrome.tabs.sendMessage(tabId, message);
    return answer ?? null;
  } catch {
    return null;
  }
}

/** Connect the native host, once per attachment. */
function openPort() {
  if (state.port !== null) {
    return;
  }
  state.port = chrome.runtime.connectNative(HOST_NAME);
  state.port.onMessage.addListener(onHostMessage);
  state.port.onDisconnect.addListener(() => {
    // Host loss ends sharing (the design note section 3.3 step 6).
    state.port = null;
    state.attached = false;
  });
  send({
    type: "hello",
    protocol: NATIVE_PROTOCOL,
    extension_id: chrome.runtime.id,
    profile_instance: PROFILE_INSTANCE,
  });
}

function onHostMessage(message) {
  switch (message && message.type) {
    case "ready":
      if (typeof message.pause_hidden_after_ms === "number") {
        state.pauseHiddenAfterMs = message.pause_hidden_after_ms;
      }
      return;
    case "request_text":
      answerText(message).catch(report);
      return;
    case "detach":
    case "refused":
      state.attached = false;
      return;
    case "navigate":
      navigate(message).catch(report);
      return;
    case "note":
      toPanel({ type: "note", note: message.note });
      return;
    case "panel":
      state.panel = message.state;
      toPanel({ type: "panel", state: message.state });
      return;
    default:
  }
}

/** Open the side panel for this tab, inside the gesture that shared it. */
function openPanel(tabId) {
  try {
    chrome.sidePanel.setOptions({ tabId, path: "src/panel.html", enabled: true });
    const opened = chrome.sidePanel.open({ tabId });
    if (opened && typeof opened.catch === "function") {
      opened.catch(report);
    }
  } catch (error) {
    report(error);
  }
}

/** Send one message to the panel, which may not be open. */
function toPanel(message) {
  try {
    const sent = chrome.runtime.sendMessage(message);
    if (sent && typeof sent.catch === "function") {
      // "Receiving end does not exist" is the ordinary case: no panel is
      // open. The host is not told, because nothing failed.
      sent.catch(() => {});
    }
  } catch {
    // The same case, on the callback form.
  }
}

/**
 * Navigate the attached tab, and acknowledge that the request was taken.
 *
 * The acknowledgement says one thing: the companion accepted the navigation.
 * Whether the page loads is a separate message, sent later, and it is never
 * folded into this one.
 */
async function navigate(request) {
  const refuse = (reason) => send({ type: "navigate_ack", request_id: request.request_id, accepted: false, reason });
  if (!state.attached || state.tabId === null) {
    refuse("not_attached");
    return;
  }
  if (state.paused) {
    refuse("paused");
    return;
  }
  if (originOf(request.url) !== state.origin) {
    refuse("origin_mismatch");
    return;
  }
  try {
    await chrome.tabs.update(state.tabId, { url: request.url });
  } catch (error) {
    report(error);
    refuse("origin_mismatch");
    return;
  }
  watchNavigation(request.request_id, request.url);
  send({ type: "navigate_ack", request_id: request.request_id, accepted: true, reason: null });
}

/** Watch one navigation until the tab settles, or until it is too late. */
function watchNavigation(requestId, url) {
  clearNavigation();
  const timer = setTimeout(() => {
    // Nothing was observed in time. `unknown` is the honest answer: this
    // companion has no permission that would let it see a load failure.
    finishNavigation("unknown");
  }, NAVIGATE_SETTLE_MS);
  state.navigation = { requestId, url, timer };
}

/** The attached tab finished a load. Say what became of the navigation. */
function settleNavigation(landedUrl) {
  if (state.navigation === null) {
    return;
  }
  finishNavigation(sameTarget(landedUrl, state.navigation.url) ? "loaded" : "unknown");
}

function finishNavigation(outcome) {
  const pending = state.navigation;
  if (pending === null) {
    return;
  }
  clearNavigation();
  send({ type: "navigate_outcome", request_id: pending.requestId, outcome });
}

function clearNavigation() {
  if (state.navigation !== null) {
    clearTimeout(state.navigation.timer);
    state.navigation = null;
  }
}

/** Whether the tab landed where the navigation asked it to go. */
function sameTarget(landed, asked) {
  try {
    const a = new URL(String(landed));
    const b = new URL(String(asked));
    a.hash = "";
    b.hash = "";
    return a.toString() === b.toString();
  } catch {
    return false;
  }
}

/** Re-probe and read the page, then answer the host. */
async function answerText(request) {
  if (state.tabId === null) {
    return;
  }
  const answer = await ask(state.tabId, { type: "extract" });
  if (answer === null) {
    return;
  }
  send({
    type: "text",
    request_id: request.request_id,
    document_id: answer.document_id,
    navigation_generation: state.generation,
    account: answer.account,
    zone: answer.zone,
    extract: answer.extract,
  });
}

function end(cause) {
  if (state.attached || state.port !== null) {
    send({ type: "detach", cause });
  }
  state.attached = false;
  state.tabId = null;
  state.origin = null;
  state.generation = 0;
  state.paused = false;
  state.panel = null;
  clearNavigation();
  if (state.port !== null) {
    state.port.disconnect();
    state.port = null;
  }
}

function send(message) {
  try {
    state.port?.postMessage(message);
  } catch {
    state.port = null;
    state.attached = false;
  }
}

/** The origin of a tab URL, or `null` when there is not one to speak of. */
function originOf(url) {
  try {
    const parsed = new URL(String(url));
    if (parsed.protocol !== "https:" && parsed.protocol !== "http:") {
      return null;
    }
    return parsed.origin;
  } catch {
    return null;
  }
}

function report(error) {
  console.warn("[canvas-cli companion]", error);
}
