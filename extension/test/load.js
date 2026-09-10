// Load the companion's pure modules into this process.
//
// The shipped files add to one global rather than exporting, because Chrome
// injects them as classic scripts. Importing them here runs exactly the code
// the browser runs, with no second copy to drift.
import "../src/shared.js";
import "../src/routes.js";
import "../src/zones.js";
import "../src/sanitize.js";
import "../src/extract.js";

export const companion = globalThis.canvasCli;

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));

/** Read one fixture from `test/fixtures/`. */
export function fixture(name) {
  return readFileSync(join(here, "fixtures", name), "utf8");
}
