// The bake worker: applies the CSS filter matrix to RGBA pixels off the main
// thread. Bundled by esbuild to public/bake.worker.js
// (tools/bundle-engine.mjs); the main thread falls back to the same kernel
// inline when no Worker is available. The shared math lives in
// ./filterKernel — the worker IS the code the fallback runs, so the two
// paths cannot drift.
//
// Two request shapes, one kernel:
// * `bitmap` — the preferred shape: the raster crosses as an ImageBitmap and
//   the PIXEL READBACK happens here, in the worker, through the worker's own
//   canvas. The main thread never pays the synchronous GPU→CPU sync a
//   getImageData forces there — that stall, once per page, was the visible
//   hitch of a multi-pane theme change.
// * `buffer` — the legacy shape: pixels read on the main thread, transferred
//   in, filtered, transferred back. Kept for runtimes without
//   createImageBitmap and as the tested reference.
// Both read the same source pixels and run the same kernel, so the two paths
// are byte-identical by construction.

import { applyFilterToData } from "./filterKernel";

type BakeRequest = {
  id: number;
  w: number;
  h: number;
  filter: string;
  buffer?: ArrayBuffer;
  bitmap?: ImageBitmap;
};

type BakeResponse = {
  id: number;
  buffer?: ArrayBuffer;
  error?: string;
};

type WorkerScope = {
  onmessage: ((ev: MessageEvent) => void) | null;
  postMessage: (msg: BakeResponse, transfer?: Transferable[]) => void;
  OffscreenCanvas?: new (w: number, h: number) => OffscreenCanvas;
};

const scope = self as unknown as WorkerScope;

scope.onmessage = (ev: MessageEvent) => {
  const req = ev.data as BakeRequest;
  try {
    let data: Uint8ClampedArray;
    if (req.bitmap) {
      const bitmap = req.bitmap;
      const Ctor = scope.OffscreenCanvas;
      if (!Ctor) throw new Error("no OffscreenCanvas in worker");
      const canvas = new Ctor(req.w, req.h);
      const ctx = canvas.getContext("2d", { alpha: false });
      if (!ctx) throw new Error("no 2d context in worker");
      ctx.drawImage(bitmap, 0, 0);
      bitmap.close();
      data = ctx.getImageData(0, 0, req.w, req.h).data;
    } else if (req.buffer) {
      data = new Uint8ClampedArray(req.buffer);
    } else {
      throw new Error("empty bake request");
    }
    applyFilterToData(data, req.w, req.h, req.filter);
    scope.postMessage({ id: req.id, buffer: data.buffer }, [data.buffer]);
  } catch (e) {
    scope.postMessage({ id: req.id, error: String(e) });
  }
};
