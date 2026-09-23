// Which format artifact a book mounts, and the order a drop follows. No DOM.
// The boot script and the web-lane test both consume this module, so a
// decision change has one place to land.
//
// The glue is not imported from these URLs. The boot script fetches the text
// and imports a blob, so the module map cannot hand back a live instance.
// These strings are the stable names of the files.

import { HOST_PDF } from "../engine/dom-contract";

export { HOST_PDF };

export type FormatId = typeof HOST_PDF | "text" | "md";

/** One handle. Phase 3 keys a map by slot id. This phase has one slot. */
export const SLOT_KEY = "slot";

/** The overview's drop order, applied to one instance. `release-binding` is
 *  the step that actually frees the wasm-bindgen heap. Nulling exports is not
 *  that step. A step that is skipped is a drop that did not happen. */
export const DROP_ORDER = [
  "flush",
  "dispose",
  "teardown",
  "release-binding",
  "null-glue",
  "drop-instance",
  "clear-handle",
] as const;

/** The shelf artifact. Not a format. It is not alive while a book is open. */
export const LIBRARY_GLUE = "sessions/library.js";
export const LIBRARY_WASM = "sessions/library_bg.wasm";

/** The reader chrome. Not a format. It is not alive on the shelf. */
export const HOST_GLUE = "sessions/host.js";
export const HOST_WASM = "sessions/host_bg.wasm";

/** Every book artifact. A library switch drops all of them, not only the slot. */
export const BOOK_FORMATS = [HOST_PDF, "text", "md"] as const;

/**
 * What a switch has to drop. `to-library` is not the format-switch condition:
 * pdf, text, and md all go, then the worker, the canvases, and the reader
 * host. `switch-book` drops only the slot. `to-reader` drops the shelf module
 * and any cover worker the shelf started.
 */
export function dropsFor(event: "to-library" | "to-reader" | "switch-book"): readonly string[] {
  if (event === "to-library") {
    return [...BOOK_FORMATS, "pdf-worker", "canvases", "reader-host"];
  }
  if (event === "to-reader") return ["library", "pdf-worker", "canvases"];
  return ["slot"];
}

/** The only session alive after a switch. The other one has been released. */
export function aliveAfter(event: "to-library" | "to-reader"): "library" | "reader-host" {
  return event === "to-library" ? "library" : "reader-host";
}

/** Unknown extensions are PDF. That is the same rule the open flow uses, so
 *  the loader and the artifact agree about which heap a file belongs in. */
export function artifactFor(path: string): FormatId {
  const slash = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  const base = slash >= 0 ? path.slice(slash + 1) : path;
  const dot = base.lastIndexOf(".");
  const ext = (dot >= 0 ? base.slice(dot + 1) : "").toLowerCase();
  if (ext === "txt" || ext === "text") return "text";
  if (ext === "md" || ext === "markdown" || ext === "mdown") return "md";
  return HOST_PDF;
}

export function gluePath(format: FormatId): string {
  return `formats/${format}.js`;
}

export function wasmPath(format: FormatId): string {
  return `formats/${format}_bg.wasm`;
}

/** Trunk's dev server answers a missing file with the app page, and that page
 *  starts with `<`. Importing it is `Unexpected token '<'` in a blob the
 *  debugger names `source`, while the title bar still shows the book. A
 *  missing artifact must fail as a missing artifact. */
export function looksLikeHtml(contentType: string | null, head: string): boolean {
  if (contentType && contentType.toLowerCase().includes("text/html")) return true;
  const start = head.replace(/^\uFEFF/, "").trimStart();
  return start.startsWith("<");
}

export type SessionModuleId = "library" | "reader-host" | FormatId;

/** Glue and wasm for one activatable module. Page modules and book modules
 *  share this so a switch does not grow a second loader. */
export function moduleGlue(id: SessionModuleId): string {
  if (id === "library") return LIBRARY_GLUE;
  if (id === "reader-host") return HOST_GLUE;
  return gluePath(id);
}

export function moduleWasm(id: SessionModuleId): string {
  if (id === "library") return LIBRARY_WASM;
  if (id === "reader-host") return HOST_WASM;
  return wasmPath(id);
}

export function isSessionModuleId(value: string): value is SessionModuleId {
  return (
    value === "library" ||
    value === "reader-host" ||
    value === HOST_PDF ||
    value === "text" ||
    value === "md"
  );
}

export function isFormatModule(id: SessionModuleId): id is FormatId {
  return id === HOST_PDF || id === "text" || id === "md";
}

/**
 * What must be disposed and released before `id` is instantiated. A book and
 * the shelf are never alive together. A format switch drops every book
 * module, including a previous instance of itself, and leaves the host up.
 * The page ids are not in their own list: the caller drops a stale instance
 * of the arriving page before creating the new one.
 */
export function releaseBefore(id: SessionModuleId): readonly SessionModuleId[] {
  if (id === "library") return [...BOOK_FORMATS, "reader-host"];
  if (id === "reader-host") return ["library", ...BOOK_FORMATS];
  return ["library", ...BOOK_FORMATS];
}
