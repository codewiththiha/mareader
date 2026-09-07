// The format-agnostic half of the browser side: today the selection
// tracker, which answers "what did the reader select, and on which page
// hosts?" for every format through the host protocol in
// public/engine/dom-contract.ts. Kept out of pdfEngine.ts so a TXT or
// Markdown selection does not depend on the bundle that carries pdf.js.
// Compiled to public/readerEngine.js and loaded by index.html before the
// wasm: the app reads selection state as soon as its first components
// mount.

export {};

import { installSelectionTracker } from "./reader/selection";

installSelectionTracker();
