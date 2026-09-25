// Search support, engine side. Matching no longer happens here: the Rust
// index (pdf-core's SearchIndex, fed by `extractPageText`) owns the query.
// This module only extracts one page's text runs for the index builder (the
// only JS work a search still does), publishes the active query so the DOM
// text layers repaint their highlight boxes (`setSearchContext`), and
// toggles the active-match emphasis or clears highlights (`setActiveMatch`,
// `clearHighlights`) — painting itself happens on the text-layer spans in
// highlights.ts.

import type { TextItem } from "./types";
import { fail, failFrom } from "./errors";
import { clearHighlightBoxes, markActiveHighlight, refreshHighlights } from "./highlights";
import { session } from "./state";

function itemRect(item: TextItem, pageH: number): { x: number; y: number; w: number; h: number } {
  const t = item.transform || [1, 0, 0, 1, 0, 0];
  const fontSize = Math.hypot(t[2] ?? 0, t[3] ?? 0);
  const ascent = (fontSize || 0) * 0.8;
  return {
    x: t[4] ?? 0,
    y: (pageH || 0) - (t[5] ?? 0) - ascent,
    w: item.width || 0,
    h: item.height || 0,
  };
}

/** One text run extracted from a page: the string and its bounding rect in
 *  scale-1 CSS px, relative to the page's top-left. */
type ExtractedPageItem = { str: string; x: number; y: number; w: number; h: number };

/** Extract `page`'s text runs, normalised to scale-1 CSS px relative to the
 *  page's top-left. `{ok:true}` with no items is a valid empty page; an
 *  unreadable page is `{ok:false, error}` — the Rust builder skips it, the
 *  same way the old streaming search did. */
export async function extractPageText(
  page: number,
): Promise<
  | { ok: true; page: number; items: ExtractedPageItem[] }
  | { ok: false; error: { name: string; message: string } }
> {
  const doc = session.pdf;
  if (!doc || page < 1) return fail("no_document", "No document open");
  // A close can land while an extraction is in flight. The awaits below go
  // to the pdf.js worker — a worker destroy() is killing — so they race the
  // document-gone signal and fail fast instead of hanging on a promise the
  // dead worker never settles (which would hold the search-build gauge up
  // forever and block the disposal baseline).
  const dying = session.documentGoneSignal();
  try {
    const pg = await Promise.race([
      doc.getPage(page),
      dying.promise.then(() => null),
    ]);
    if (!pg) return fail("no_document", "Document closed mid-extraction");
    const tc = await Promise.race([
      pg.getTextContent(),
      dying.promise.then(() => null),
    ]);
    if (!tc) return fail("no_document", "Document closed mid-extraction");
    const pageH = pg.getViewport({ scale: 1 }).height;
    const items: ExtractedPageItem[] = [];
    for (const item of tc.items) {
      if (!item.str) continue;
      const r = itemRect(item, pageH);
      if (r.w <= 0) continue; // no rectangle → nothing to highlight
      items.push({ str: item.str, x: r.x, y: r.y, w: r.w, h: r.h });
    }
    try {
      pg.cleanup();
    } catch (_) {
      /* already cleaned */
    }
    return { ok: true, page, items };
  } catch (e) {
    return failFrom(e);
  } finally {
    dying.unsubscribe();
  }
}

/** Publish the active query so mounted text layers repaint their highlight
 *  boxes immediately (they paint from the DOM spans, not from the match
 *  list, so the index result alone would leave the page unmarked). */
export function setSearchContext(query: string): void {
  session.setSearchQuery(String(query || "").toLowerCase().trim());
  refreshHighlights();
}

export function setActiveMatch(page: number, index: number): void {
  const next =
    Number.isFinite(page) && page > 0 && Number.isFinite(index) && index >= 0
      ? { page, index: index | 0 }
      : null;
  session.setActiveMatchValue(next);
  for (const st of session.stateByCanvasId.values()) markActiveHighlight(st);
}

export function clearHighlights(): void {
  session.setSearchQuery("");
  session.setActiveMatchValue(null);
  for (const st of session.stateByCanvasId.values()) clearHighlightBoxes(st);
}
