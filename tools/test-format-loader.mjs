// The format-loader decisions, against the module bundle-engine emits. Run
// after `npm run build:ts`. Node, not a browser: the module has no DOM.

import assert from "node:assert/strict";
import { DROP_ORDER, SLOT_KEY, artifactFor, gluePath, wasmPath } from "../scripts/format-loader.js";

assert.equal(SLOT_KEY, "slot");
assert.deepEqual(DROP_ORDER, [
  "flush",
  "dispose",
  "teardown",
  "null-glue",
  "drop-instance",
  "clear-handle",
]);
assert.equal(DROP_ORDER.includes("keep-instance"), false);

assert.equal(artifactFor("/tmp/a.pdf"), "pdf");
assert.equal(artifactFor("/tmp/a.PDF"), "pdf");
assert.equal(artifactFor("C:\\books\\a.PDF"), "pdf");
assert.equal(artifactFor("/tmp/noext"), "pdf");
assert.equal(artifactFor("/tmp/a.txt"), "text");
assert.equal(artifactFor("/tmp/a.text"), "text");
assert.equal(artifactFor("/tmp/a.TXT"), "text");
assert.equal(artifactFor("/tmp/a.md"), "md");
assert.equal(artifactFor("/tmp/a.markdown"), "md");
assert.equal(artifactFor("/tmp/a.mdown"), "md");
assert.equal(artifactFor("/tmp/a.MD"), "md");
assert.equal(artifactFor("notes.md.txt"), "text");

assert.equal(gluePath("pdf"), "formats/pdf.js");
assert.equal(gluePath("text"), "formats/text.js");
assert.equal(gluePath("md"), "formats/md.js");
assert.equal(wasmPath("pdf"), "formats/pdf_bg.wasm");
assert.equal(wasmPath("text"), "formats/text_bg.wasm");
assert.equal(wasmPath("md"), "formats/md_bg.wasm");

console.log("format loader ok");
