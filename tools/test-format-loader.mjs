// The format-loader decisions, against the module bundle-engine emits. Run
// after `npm run build:ts`. Node, not a browser: the module has no DOM.

import assert from "node:assert/strict";
import {
  BOOK_FORMATS,
  DROP_ORDER,
  HOST_GLUE,
  HOST_WASM,
  LIBRARY_GLUE,
  LIBRARY_WASM,
  SLOT_KEY,
  aliveAfter,
  artifactFor,
  dropsFor,
  gluePath,
  isFormatModule,
  isSessionModuleId,
  looksLikeHtml,
  moduleGlue,
  moduleWasm,
  releaseBefore,
  wasmPath,
} from "../scripts/format-loader.js";

assert.equal(SLOT_KEY, "slot");
assert.deepEqual(DROP_ORDER, [
  "flush",
  "dispose",
  "teardown",
  "release-binding",
  "null-glue",
  "drop-instance",
  "clear-handle",
]);
assert.equal(DROP_ORDER.includes("release-binding"), true);
assert.equal(DROP_ORDER.includes("keep-instance"), false);

assert.deepEqual(BOOK_FORMATS, ["pdf", "text", "md"]);
assert.equal(LIBRARY_GLUE, "sessions/library.js");
assert.equal(LIBRARY_WASM, "sessions/library_bg.wasm");
assert.deepEqual(dropsFor("to-library"), ["pdf", "text", "md", "pdf-worker", "canvases", "reader-host"]);
assert.deepEqual(dropsFor("to-reader"), ["library", "pdf-worker", "canvases"]);
assert.deepEqual(dropsFor("switch-book"), ["slot"]);
assert.equal(dropsFor("to-library").includes("pdf"), true);
assert.equal(dropsFor("to-library").includes("text"), true);
assert.equal(dropsFor("to-library").includes("md"), true);
assert.equal(dropsFor("switch-book").includes("md"), false);
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

assert.equal(looksLikeHtml("text/html", "export"), true);
assert.equal(looksLikeHtml("text/html; charset=utf-8", ""), true);
assert.equal(looksLikeHtml(null, "<!doctype html>\n"), true);
assert.equal(looksLikeHtml(null, "\n  <html>"), true);
assert.equal(looksLikeHtml("application/javascript", "export default function"), false);
assert.equal(looksLikeHtml(null, "export async function mount"), false);

assert.deepEqual(releaseBefore("library"), ["pdf", "text", "md", "reader-host"]);
assert.deepEqual(releaseBefore("reader-host"), ["library", "pdf", "text", "md"]);
assert.deepEqual(releaseBefore("pdf"), ["library", "pdf", "text", "md"]);
assert.deepEqual(releaseBefore("text"), ["library", "pdf", "text", "md"]);
assert.deepEqual(releaseBefore("md"), ["library", "pdf", "text", "md"]);
assert.equal(releaseBefore("library").includes("library"), false);
assert.equal(releaseBefore("text").includes("reader-host"), false);
assert.equal(releaseBefore("md").includes("reader-host"), false);
assert.equal(moduleGlue("library"), LIBRARY_GLUE);
assert.equal(moduleWasm("library"), LIBRARY_WASM);
assert.equal(moduleGlue("reader-host"), HOST_GLUE);
assert.equal(moduleWasm("reader-host"), HOST_WASM);
assert.equal(moduleGlue("text"), "formats/text.js");
assert.equal(moduleWasm("md"), "formats/md_bg.wasm");
assert.equal(moduleGlue("pdf"), gluePath("pdf"));
assert.equal(isFormatModule("text"), true);
assert.equal(isFormatModule("md"), true);
assert.equal(isFormatModule("library"), false);
assert.equal(isSessionModuleId("reader-host"), true);
assert.equal(isSessionModuleId("epub"), false);

console.log("format loader ok");

