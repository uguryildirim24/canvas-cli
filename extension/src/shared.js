// The namespace every companion file shares.
//
// These files are loaded two ways and must work as both: Chrome injects them
// into the isolated world as classic scripts (`scripting.executeScript` takes
// files, not modules), and `node --test` imports them as ES modules. Neither
// `export` nor `require` would work in both, so each file adds to one global
// object and nothing here imports anything.
globalThis.canvasCli = globalThis.canvasCli || {};

// The version both ends of the native channel declare.
globalThis.canvasCli.NATIVE_PROTOCOL = "bridge-native@1";

// The total browser payload ceiling, in UTF-8 bytes (REPORT section 3.3).
globalThis.canvasCli.MAX_PAYLOAD_BYTES = 64 * 1024;

// The one Canvas request this companion ever makes.
globalThis.canvasCli.PROBE_PATH = "/api/v1/users/self";
