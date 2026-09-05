// The bake worker: applies the CSS filter matrix to RGBA pixels off the main
// thread. Bundled by esbuild to public/bake.worker.js (see
// tools/bundle-engine.mjs); the main thread falls back to the same kernel
// inline when no Worker is available (Node smoke runs, exotic webviews).
//
// The shared math lives in ./filterKernel — the worker IS the same code the
// fallback runs, so the two paths cannot drift.

import { applyFilterToData } from "./filterKernel";

type BakeRequest = {
  id: number;
  w: number;
  h: number;
  filter: string;
  buffer: ArrayBuffer;
};

type BakeResponse = {
  id: number;
  buffer?: ArrayBuffer;
  error?: string;
};

const scope = self as unknown as {
  onmessage: ((ev: MessageEvent) => void) | null;
  postMessage: (msg: BakeResponse, transfer?: Transferable[]) => void;
};

scope.onmessage = (ev: MessageEvent) => {
  const req = ev.data as BakeRequest;
  const data = new Uint8ClampedArray(req.buffer);
  try {
    applyFilterToData(data, req.w, req.h, req.filter);
    scope.postMessage({ id: req.id, buffer: data.buffer }, [data.buffer]);
  } catch (e) {
    scope.postMessage({ id: req.id, error: String(e) });
  }
};
