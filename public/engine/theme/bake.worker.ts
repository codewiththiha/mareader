// The bake worker: filterKernel off the main thread; the same code runs
// inline without one.

import { applyFilterToData } from "./filterKernel";

type BakeRequest = {
  id: number;
  w: number;
  h: number;
  filter: string;
  bitmap: ImageBitmap;
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
    const bitmap = req.bitmap;
    const Ctor = scope.OffscreenCanvas;
    if (!Ctor) throw new Error("no OffscreenCanvas in worker");
    const canvas = new Ctor(req.w, req.h);
    const ctx = canvas.getContext("2d", { alpha: false });
    if (!ctx) throw new Error("no 2d context in worker");
    ctx.drawImage(bitmap, 0, 0);
    bitmap.close();
    const data = ctx.getImageData(0, 0, req.w, req.h).data;
    applyFilterToData(data, req.w, req.h, req.filter);
    scope.postMessage({ id: req.id, buffer: data.buffer }, [data.buffer]);
  } catch (e) {
    scope.postMessage({ id: req.id, error: String(e) });
  }
};
