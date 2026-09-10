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
// - the pauses: hidden tab, assessment, tab close, host loss.
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

/** @type {{port: chrome.runtime.Port|null, tabId: number|null, origin: string|null,
 *          generation: number, pauseHiddenAfterMs: number, attached: boolean}} */
const state = {
  port: null,
  tabId: null,
  origin: null,
  generation: 0,
  pauseHiddenAfterMs: 10 * 60 * 1000,
  attached: false,
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
  observe().catch(report);
});

chrome.runtime.onMessage.addListener((message) => {
  if (message && message.type === "pause") {
    send({ type: "pause", cause: message.cause || "hidden" });
  }
  return false;
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
  openPort();
  await inject(tab.id);
  const observation = await ask(tab.id, { type: "observe", pause_hidden_after_ms: state.pauseHiddenAfterMs });
  if (observation === null) {
    return;
  }
  state.attached = true;
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
    // Host loss ends sharing (REPORT section 3.3 step 6).
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
    default:
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
