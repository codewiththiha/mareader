// =====================================================================
// readerEngine.ts — the format-agnostic half of the browser side.
//
// Today that is one thing: the selection tracker, which answers "what did
// the reader select, and on which page hosts?" for every format through the
// host protocol in `public/engine/dom-contract.ts`. It used to be installed
// by `pdfEngine.ts`, which made a TXT or Markdown document's selection depend
// on the bundle that also carries pdf.js.
//
// Compiled to public/readerEngine.js and loaded by index.html as a module
// script before the wasm, for the same reason the engine bundle is: the app
// reads selection state as soon as its first components mount.
// =====================================================================

export {};

import { installSelectionTracker } from "./reader/selection";

installSelectionTracker();
