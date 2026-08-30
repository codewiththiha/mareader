# Vendored pdf.js

This directory holds the pdf.js build the app actually loads. `index.html` imports
`/vendor/pdfjs/pdf.min.mjs` as an ESM module (pdf.js is ESM-only in v6) and the
engine talks to `globalThis.pdfjsLib`; the worker is spawned from
`pdf.worker.min.mjs`, `pdf_viewer.css` supplies the text-layer styles, and
`cmaps/` covers CJK mappings.

Nothing here is installed or copied at build time: `npm ci` has **no**
`pdfjs-dist` dependency, so `node_modules` and the wasm bundle stay out of the
loop. The files were cut from `pdfjs-dist@6.2.108`.

To refresh the vendored copy:

1. `npm pack pdfjs-dist@<version>` (or install it temporarily — do not add it
   back to `package.json`),
2. replace `pdf.min.mjs` with `build/pdf.min.mjs`, `pdf.worker.min.mjs` with
   `build/pdf.worker.min.mjs`, `pdf_viewer.css` with `web/pdf_viewer.css` and
   `cmaps/` with `cmaps/`,
3. keep the file names — they are referenced from `index.html` and from the
   engine's worker setup.

The version is visible in `pdf.min.mjs` (`6.2.108` today); bump it here when the
build is replaced so the provenance stays checkable.
