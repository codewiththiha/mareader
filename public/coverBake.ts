// The Shell's cover-bake page: shelf covers rendered without a reader.
//
// `bake.html` is a child document the Shell mounts (src/app/bake.rs) when
// the library asks for covers it lacks, and removes once the queue has
// drained. It loads pdf.js and this script — no wasm, no runtime, no
// session — so a shelf can get its covers while nothing but the shell and
// the library is resident. Before this page existed the Shell relayed each
// bake to a READER frame, which was the second reason (after warming) a
// reader stayed booted behind the shelf.
//
// The wire is three window messages, same origin, parent ⇄ this frame:
//   this → parent  {kind:"mareader.bake-ready"}                  once, on load
//   parent → this  {kind:"mareader.bake", id, path, width}        one per cover
//   this → parent  {kind:"mareader.baked", id, path, ok,
//                   dataUrl?, width?, height?, error?}            one per ask
// Every ask is answered — `ok:false` on any failure — so the Shell's
// in-flight ledger never waits on silence (its per-bake timeout is the
// backstop for a page that dies mid-render, not the normal failure path).
//
// The render itself is the engine's own `coverDataUrl` (engine/loader.ts):
// the same standalone loading task the reader uses for a cover, torn down
// before the answer goes out, so a bake never leaves a pdf.js worker
// behind. Local files come in over the Tauri IPC that tauri-relay.js
// republishes on this window, exactly as in the reader frame.

import { coverDataUrl } from "./engine/loader";
import { EngineSession } from "./engine/state";

/** Each ask renders in its own throwaway engine session: the cover's
 *  loading task (and its worker) is counted on, and destroyed inside, the
 *  one call — nothing outlives the answer, and no two asks share state.
 *  The page has no facade, so these sessions are never registered. */
let nextBakeSid = 1;
let disposed = false;
let active: AbortController | undefined;
let listener: ((event: MessageEvent) => void) | undefined;

declare global {
  interface Window { __mareaderDisposeBakes?: () => void; }
}

/** One bake at a time, owned by this page. The Shell calls this before
 * removing the iframe; abort releases the offscreen canvas synchronously. */
function disposeBakes(): void {
  if (disposed) return;
  disposed = true;
  active?.abort();
  active = undefined;
  if (listener) window.removeEventListener("message", listener);
  window.removeEventListener("pagehide", disposeBakes);
  delete window.__mareaderDisposeBakes;
  listener = undefined;
}

type Ask = { kind: "mareader.bake"; id: number; path: string; width: number };

/** The parent's origin for every post: our own, since the page is
 *  same-origin by construction. `"null"` (an opaque origin, e.g. file://)
 *  cannot be named, so only then does the post fall back to `*`. */
function parentOrigin(): string {
  const origin = window.location.origin;
  return origin && origin !== "null" ? origin : "*";
}

function isAsk(value: unknown): value is Ask {
  if (!value || typeof value !== "object") return false;
  const v = value as Partial<Ask>;
  return v.kind === "mareader.bake" && typeof v.id === "number" && typeof v.path === "string";
}

async function bake(target: Window, ask: Ask): Promise<void> {
  if (disposed || active) return;
  const controller = new AbortController();
  active = controller;
  const width = typeof ask.width === "number" && ask.width > 0 ? ask.width : 240;
  let answer: Record<string, unknown>;
  try {
    const result = await coverDataUrl(new EngineSession(nextBakeSid++), ask.path, width, controller.signal);
    answer = result.ok
      ? {
          kind: "mareader.baked",
          id: ask.id,
          path: ask.path,
          ok: true,
          dataUrl: result.dataUrl,
          width: result.width,
          height: result.height,
        }
      : {
          kind: "mareader.baked",
          id: ask.id,
          path: ask.path,
          ok: false,
          error: `${result.error.name}: ${result.error.message}`,
        };
  } catch (e) {
    answer = {
      kind: "mareader.baked",
      id: ask.id,
      path: ask.path,
      ok: false,
      error: String(e),
    };
  }
  if (active === controller) active = undefined;
  if (!disposed) target.postMessage(answer, parentOrigin());
}

function main(): void {
  const parent = window.parent;
  // Opened on its own (no Shell above it) the page is inert: nobody can ask
  // it for anything, and answering `window` itself would be talking to no one.
  if (!parent || parent === window) return;
  window.__mareaderDisposeBakes = disposeBakes;
  window.addEventListener("pagehide", disposeBakes, { once: true });
  listener = (event: MessageEvent) => {
    // Only the document that mounted this page may ask it to read files.
    if (event.source !== parent) return;
    const origin = window.location.origin;
    if (origin && origin !== "null" && event.origin !== origin) return;
    if (!isAsk(event.data)) return;
    void bake(parent, event.data);
  };
  window.addEventListener("message", listener);
  parent.postMessage({ kind: "mareader.bake-ready" }, parentOrigin());
}

main();
