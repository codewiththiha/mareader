// The representative page: one shared miniature that stands in for every page
// the reader is not owed a raster for.
//
// A placeholder tier is only free if it is actually shared. The obvious
// implementation — give each placeholder page its own canvas and draw
// something into it — costs a backing store, a 2d context and a GPU texture
// per page, which for a window of eight mounted pages is eight surfaces that
// exist to look like nothing in particular. The tier is supposed to be the one
// that costs almost zero.
//
// So there is exactly ONE placeholder raster per document, published as a CSS
// custom property and painted by the stylesheet into as many boxes as are
// mounted (`--pdf-placeholder`, styles/page_host.css). The image is decoded
// once and every box that references it shares the decode.
//
// Which page represents the document is a measurement, not a guess. Page 1 is
// the wrong answer often enough to matter: it is a cover, a frontispiece, a
// landscape plate in an otherwise portrait book. The pages are sampled, bucketed
// by aspect ratio, and the heaviest bucket's middle page is rendered once at a
// resolution no bigger than a thumbnail — a shape the whole document agrees
// with, rather than the shape its first page happens to have.

import {
  isSharedScratch,
  offscreenFor,
  releaseCanvas,
  releasePooledCanvas,
  releaseScratch,
} from "./canvas";
import { bakeRaster } from "./theme/bake";
import { readPipeline } from "./theme/pipeline";
import { session } from "./state";

/** The CSS custom property the placeholder boxes paint from. */
export const PLACEHOLDER_PROPERTY = "--pdf-placeholder";

/** The miniature's pixel ceiling. ~400×450 for a portrait page: enough that a
 *  blurred, low-opacity copy still reads as "a page of this document", and
 *  small enough that the surface costs under a megabyte. */
const PLACEHOLDER_MAX_PIXELS = 180_000;

/** How many pages to measure when picking the representative. Spread across
 *  the document rather than taken from the front, because the front is where
 *  the unrepresentative pages live. */
const SAMPLE_PAGES = 12;

/** Aspect ratios closer than this are the same shape. A book's pages are cut
 *  to a standard, so the buckets are sharp in practice; the tolerance is for
 *  the scan that is a millimetre off. */
const ASPECT_EPSILON = 0.02;

/** How long after an open the sampling starts. The reader's first page is
 *  queued ahead of it and must stay that way: this is background work for a
 *  scroll that has not happened yet. */
const SAMPLE_DELAY_MS = 220;

/** Encode quality for the published image. It is painted blurred at a fifth
 *  of its opacity; artifacts in it are not visible, and the bytes are. */
const ENCODE_QUALITY = 0.6;

let publishedUrl: string | null = null;
let revokeOnReset: string | null = null;
let rawPlaceholder: HTMLCanvasElement | null = null;
let scheduled = false;

/** The published URL, or null when there is no miniature yet. */
export function placeholderUrl(): string | null {
  return publishedUrl;
}

/** Encode a canvas to a URL the stylesheet can paint. `toBlob` + an object URL
 *  keeps the bytes off the style attribute and hands the encode to the
 *  browser's own thread; the data URL fallback is for platforms without
 *  either (and for the smoke harness's stub canvas). */
function encode(canvas: HTMLCanvasElement): Promise<string | null> {
  const sync = (): string | null => {
    try {
      return canvas.toDataURL("image/jpeg", ENCODE_QUALITY);
    } catch (_) {
      return null;
    }
  };
  if (typeof canvas.toBlob !== "function" || !objectUrls()) {
    return Promise.resolve(sync());
  }
  return new Promise<string | null>((resolve) => {
    canvas.toBlob(
      (blob) => {
        if (!blob) return resolve(sync());
        try {
          resolve(URL.createObjectURL(blob));
        } catch (_) {
          resolve(sync());
        }
      },
      "image/jpeg",
      ENCODE_QUALITY,
    );
  });
}

/** Whether object URLs exist here. Spelled as a `typeof URL` guard rather
 *  than `URL?.createObjectURL` because the optional chain resolves the
 *  binding first, and an undeclared `URL` (the smoke harness's sandbox) is a
 *  ReferenceError rather than an undefined. */
function objectUrls(): boolean {
  return typeof URL !== "undefined" && typeof URL.createObjectURL === "function";
}

/** Publish (or, with no argument, clear) the property the boxes paint from. */
function publish(url: string | null): void {
  const isObjectUrl = !!url && url.startsWith("blob:");
  if (revokeOnReset && revokeOnReset !== url) {
    try {
      if (typeof URL !== "undefined" && typeof URL.revokeObjectURL === "function") {
        URL.revokeObjectURL(revokeOnReset);
      }
    } catch (_) {
      /* already revoked */
    }
  }
  revokeOnReset = isObjectUrl ? url : null;
  publishedUrl = url;
  try {
    const root = document.documentElement;
    if (url) root.style.setProperty(PLACEHOLDER_PROPERTY, `url("${url}")`);
    else root.style.removeProperty(PLACEHOLDER_PROPERTY);
  } catch (_) {
    /* no document (the smoke harness publishes through its stub) */
  }
}

/** Re-bake and re-publish from the retained raw. Called by the theme path: a
 *  miniature baked under Light is a white rectangle in Dark, and the
 *  placeholder boxes are on screen exactly when the reader is scrolling — so
 *  the tier that is supposed to look like the document cannot be the one part
 *  of it that ignores the theme. */
export async function republishPlaceholder(): Promise<void> {
  if (!rawPlaceholder) return;
  const baked = await bakeRaster(rawPlaceholder, readPipeline());
  const url = await encode(baked);
  // A bake's intermediate is the shared scratch (a filter-only bake) or a
  // pooled canvas, and returning the scratch to the pool would give one canvas
  // two owners — the same rule renderer.ts's own `releaseBaked` follows.
  if (baked !== rawPlaceholder) {
    if (isSharedScratch(baked)) releaseScratch(baked);
    else releasePooledCanvas(baked);
  }
  publish(url);
}

/** Which page represents the document: the middle page of the aspect ratio
 *  most pages share. Pure over the samples, so the choice is testable without
 *  a document. */
export function representativePage(
  samples: { page: number; width: number; height: number }[],
): number | null {
  const measured = samples.filter(
    (sample) => sample.width > 0 && sample.height > 0 && sample.page > 0,
  );
  if (measured.length === 0) return null;
  const buckets: { aspect: number; pages: number[] }[] = [];
  for (const sample of measured) {
    const aspect = sample.width / sample.height;
    const bucket = buckets.find((entry) => Math.abs(entry.aspect - aspect) <= ASPECT_EPSILON);
    if (bucket) bucket.pages.push(sample.page);
    else buckets.push({ aspect, pages: [sample.page] });
  }
  let heaviest = buckets[0];
  for (const bucket of buckets) {
    if (bucket.pages.length > heaviest.pages.length) heaviest = bucket;
  }
  const sorted = [...heaviest.pages].sort((a, b) => a - b);
  // The middle of the bucket, not its first: the first page of a shape is
  // where a cover or a section opener sits.
  return sorted[Math.floor(sorted.length / 2)] ?? null;
}

/** The scale that lands a page inside the miniature's pixel ceiling. */
function miniatureScale(width: number, height: number): number {
  const pixels = width * height;
  if (!(pixels > 0)) return 0;
  if (pixels <= PLACEHOLDER_MAX_PIXELS) return 1;
  return Math.sqrt(PLACEHOLDER_MAX_PIXELS / pixels);
}

/** Measure the sampled pages and render the representative once. Every step is
 *  best-effort: a document that will not yield a miniature simply has no
 *  placeholder image, and the boxes fall back to the paper colour they already
 *  carry. */
async function build(): Promise<void> {
  const pdf = session.pdf;
  if (!pdf) return;
  const numPages = session.numPages;
  if (!(numPages > 0)) return;

  const count = Math.min(SAMPLE_PAGES, numPages);
  const stride = numPages / count;
  const samples: { page: number; width: number; height: number }[] = [];
  for (let i = 0; i < count; i += 1) {
    const page = Math.min(numPages, Math.max(1, Math.round(i * stride + stride / 2)));
    try {
      const proxy = await pdf.getPage(page);
      const viewport = proxy.getViewport({ scale: 1 });
      try { proxy.cleanup(); } catch (_) { /* ignore */ }
      samples.push({ page, width: viewport.width, height: viewport.height });
    } catch (_) {
      /* an unreadable page is simply not a sample */
    }
  }
  const representative = representativePage(samples);
  if (!representative || !session.pdf || session.pdf !== pdf) return;

  const size = samples.find((sample) => sample.page === representative);
  const scale = miniatureScale(size?.width ?? 0, size?.height ?? 0);
  if (!(scale > 0)) return;
  try {
    const proxy = await pdf.getPage(representative);
    // The document can have been torn down while the samples were in flight.
    if (!session.pdf || session.pdf !== pdf) {
      try { proxy.cleanup(); } catch (_) { /* ignore */ }
      return;
    }
    const viewport = proxy.getViewport({ scale });
    const made = offscreenFor(viewport);
    if (!made) {
      try { proxy.cleanup(); } catch (_) { /* ignore */ }
      return;
    }
    const task = proxy.render({ canvasContext: made.ctx, viewport });
    try {
      await task.promise;
    } catch (_) {
      releaseCanvas(made.canvas);
      try { proxy.cleanup(); } catch (_) { /* ignore */ }
      return;
    }
    try { proxy.cleanup(); } catch (_) { /* ignore */ }
    if (!session.pdf || session.pdf !== pdf) {
      releaseCanvas(made.canvas);
      return;
    }
    if (rawPlaceholder) releaseCanvas(rawPlaceholder);
    rawPlaceholder = made.canvas;
    await republishPlaceholder();
  } catch (_) {
    /* best-effort: no miniature, paper colour only */
  }
}

/** Start building the document's miniature. Fire-and-forget, after a beat: the
 *  reader's first page owns the worker until it has painted. */
export function schedulePlaceholder(): void {
  if (scheduled) return;
  scheduled = true;
  const start = (): void => {
    void build().catch(() => {
      /* best-effort */
    });
  };
  if (typeof setTimeout === "function") setTimeout(start, SAMPLE_DELAY_MS);
  else start();
}

/** Forget the document's miniature: teardown and open both call it, so a
 *  placeholder from the last book can never stand in for this one. */
export function resetPlaceholder(): void {
  scheduled = false;
  if (rawPlaceholder) {
    releaseCanvas(rawPlaceholder);
    rawPlaceholder = null;
  }
  publish(null);
}
