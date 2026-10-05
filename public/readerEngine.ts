// The format-agnostic half of the browser side: today the selection
// tracker, which answers "what did the reader select, and on which page
// hosts?" for every format through the host protocol in
// public/engine/dom-contract.ts. Kept out of pdfEngine.ts so a TXT or
// Markdown selection does not depend on the bundle that carries pdf.js.
// Compiled to public/readerEngine.js and loaded by the document-pane pages
// before their WASM, never by the persistent Shell or workspace host.

export {};

import { installSelectionTracker } from "./reader/selection";

installSelectionTracker();
