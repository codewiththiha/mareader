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

/** The overview's drop order, applied to the one slot. The boot script walks
 *  this. A step that is skipped is a drop that did not happen. */
export const DROP_ORDER = [
  "flush",
  "dispose",
  "teardown",
  "null-glue",
  "drop-instance",
  "clear-handle",
] as const;

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
