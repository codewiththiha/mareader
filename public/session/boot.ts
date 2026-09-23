// Runs before the Trunk wasm module. Module scripts are ordered, and this
// file top-level-awaits, so the wasm module does not evaluate until the boot
// object exists and — for a reader — the engines have been imported.
//
// Specifiers are variables on purpose. A literal import would let the bundler
// inline pdf.js into this file, which is the opposite of "the shelf does not
// load pdf.js".

import {
  HANDOFF_KEY,
  decideSession,
  navigation,
  parseHandoff,
  withSession,
  type Handoff,
  type HistoryMode,
} from "./handoff";

const PDFJS = "/vendor/pdfjs/pdf.min.mjs";
const PDF_ENGINE = "/pdfEngine.js";
const READER_ENGINE = "/readerEngine.js";

interface Boot {
  kind: "library" | "reader";
  open: Handoff | null;
  takeOpen(): Handoff | null;
  enterReader(payload: Handoff): void;
  enterLibrary(): void;
  clearHandoff(): void;
  ensureEngine(): Promise<void>;
  flush(): void;
}

declare global {
  interface Window {
    __MAREADER_BOOT?: Boot;
  }
}

function engineLoaded(): boolean {
  return typeof (globalThis as { PDFReader?: unknown }).PDFReader !== "undefined";
}

async function load(url: string): Promise<void> {
  await import(url);
}

let engineReady: Promise<void> | null = null;

function loadPdfEngine(): Promise<void> {
  if (engineLoaded()) return Promise.resolve();
  if (!engineReady) {
    engineReady = (async () => {
      await load(PDFJS);
      await load(PDF_ENGINE);
    })();
  }
  return engineReady;
}

function go(next: string, prefer: "push" | "replace"): void {
  const mode: HistoryMode = navigation(location.href, next, prefer);
  if (mode === "reload") location.reload();
  else if (mode === "push") location.assign(next);
  else location.replace(next);
}

const raw = sessionStorage.getItem(HANDOFF_KEY);
const kind = decideSession(location.search, raw);

// A reader URL with nothing to open is not a reader. Hang this module so the
// wasm script behind it never starts on a page that is already leaving.
if (new URLSearchParams(location.search).get("session") === "reader" && kind !== "reader") {
  location.replace(withSession(location.href, "library"));
  await new Promise(() => {});
}

if (kind === "reader") {
  try {
    await loadPdfEngine();
    await load(READER_ENGINE);
  } catch (err) {
    console.error("[boot] reader engines failed to load", err);
  }
}

const boot: Boot = {
  kind,
  open: kind === "reader" ? parseHandoff(raw) : null,
  takeOpen() {
    const value = this.open;
    this.open = null;
    sessionStorage.removeItem(HANDOFF_KEY);
    return value;
  },
  enterReader(payload) {
    if (!payload || typeof payload.path !== "string" || payload.path.length === 0) return;
    const stored: Handoff =
      typeof payload.bookId === "string" && payload.bookId.length > 0
        ? { path: payload.path, bookId: payload.bookId }
        : { path: payload.path };
    sessionStorage.setItem(HANDOFF_KEY, JSON.stringify(stored));
    // Push, so Back from the reader loads the shelf as a new document.
    go(withSession(location.href, "reader"), "push");
  },
  enterLibrary() {
    sessionStorage.removeItem(HANDOFF_KEY);
    this.open = null;
    // Replace, so Back from the shelf does not restore a reader URL.
    go(withSession(location.href, "library"), "replace");
  },
  clearHandoff() {
    sessionStorage.removeItem(HANDOFF_KEY);
    this.open = null;
  },
  ensureEngine() {
    return loadPdfEngine();
  },
  flush() {},
};

window.__MAREADER_BOOT = boot;

// pagehide is the flush. unload is the back-forward-cache opt-out: a cached
// document is still alive, and a living document still holds the wasm heap.
window.addEventListener("pagehide", () => {
  window.__MAREADER_BOOT?.flush();
});
window.addEventListener("unload", () => {
  window.__MAREADER_BOOT?.flush();
});
