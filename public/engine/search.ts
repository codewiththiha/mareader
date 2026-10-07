// Search, engine side: text extraction for the Rust index and highlight
// toggles.

import type { TextItem } from "./types";
import { fail, failFrom } from "./errors";
import { clearHighlightBoxes, markActiveHighlight, refreshHighlights } from "./highlights";
import type { EngineSession } from "./state";

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

// One text run: the string and its rect in scale-1 CSS px.
type ExtractedPageItem = { str: string; x: number; y: number; w: number; h: number };

// Extract `page`'s text runs, normalised to scale-1 CSS px.
export async function extractPageText(
  s: EngineSession,
  page: number,
): Promise<
  | { ok: true; page: number; items: ExtractedPageItem[] }
  | { ok: false; error: { name: string; message: string } }
> {
  const doc = s.pdf;
  if (!doc || page < 1) return fail("no_document", "No document open");
  // A close can land mid-extraction: the awaits race document-gone.
  const dying = s.documentGoneSignal();
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

// Publish the active query so text layers repaint.
export function setSearchContext(s: EngineSession, query: string): void {
  s.setSearchQuery(String(query || "").toLowerCase().trim());
  refreshHighlights(s);
}

export function setActiveMatch(s: EngineSession, page: number, index: number): void {
  const next =
    Number.isFinite(page) && page > 0 && Number.isFinite(index) && index >= 0
      ? { page, index: index | 0 }
      : null;
  s.setActiveMatchValue(next);
  for (const st of s.stateByCanvasId.values()) markActiveHighlight(s, st);
}

export function clearHighlights(s: EngineSession): void {
  s.setSearchQuery("");
  s.setActiveMatchValue(null);
  for (const st of s.stateByCanvasId.values()) clearHighlightBoxes(st);
}
