// The Shell's cover-bake page: shelf covers rendered without a reader.

import { coverDataUrl } from "./engine/loader";
import { EngineSession } from "./engine/state";

// Each ask renders in its own throwaway engine session.
let nextBakeSid = 1;
let disposed = false;
let active: AbortController | undefined;
let listener: ((event: MessageEvent) => void) | undefined;

declare global {
  interface Window { __mareaderDisposeBakes?: () => void; }
}

// One bake at a time, owned by this page.
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

// The parent's origin for every post.
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
  // Opened on its own the page is inert.
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
