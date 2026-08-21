// On-demand streaming search. Pages are extracted as we scan and then
// discarded — we do not keep a permanent Map of every TextItem in the heap.

import type { SearchMatch, SearchRect, SearchResult, TextItem } from "./types";
import { fail } from "./canvas";
import { refreshHighlights } from "./highlights";
import {
  highlightsByPage,
  numPages,
  pdf,
  searchQuery,
  setActiveMatchValue,
  setHighlightModeValue,
  setSearchQuery,
  stateByCanvasId,
  textIndex,
} from "./state";

function itemRect(item: TextItem, pageH: number): SearchRect {
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

function snippetText(str: string, q: string, from: number | undefined): string {
  const idx = from === undefined ? str.toLowerCase().indexOf(q) : from;
  const start = Math.max(0, idx - 25);
  const end = Math.min(str.length, idx + q.length + 30);
  const pre = start > 0 ? "…" : "";
  const post = end < str.length ? "…" : "";
  return pre + str.slice(start, end) + post;
}

/** No-op: search() streams pages itself. Kept so Rust's existing call still resolves. */
export async function buildSearchIndex(): Promise<number> {
  textIndex.clear();
  return 0;
}

export async function search(query: string): Promise<SearchResult> {
  if (!pdf) return fail("no_document", "No document open");
  const q = String(query || "").toLowerCase().trim();
  if (!q) {
    setSearchQuery("");
    setActiveMatchValue(null);
    highlightsByPage.clear();
    return { ok: true, query: "", total: 0, matches: [] };
  }

  setSearchQuery(q);
  setHighlightModeValue("live");
  highlightsByPage.clear();
  const matches: SearchMatch[] = [];
  const qlen = q.length;

  for (let page = 1; page <= numPages; page += 1) {
    try {
      const pg = await pdf.getPage(page);
      const tc = await pg.getTextContent();
      const pageH = pg.getViewport({ scale: 1 }).height;
      const pageMatches: SearchRect[] = [];
      let ord = 0;
      for (const item of tc.items) {
        if (!item.str) continue;
        const r = itemRect(item, pageH);
        if (r.w <= 0) continue;
        const lower = item.str.toLowerCase();
        const len = lower.length || 1;
        for (
          let at = lower.indexOf(q);
          at !== -1;
          at = lower.indexOf(q, at + qlen)
        ) {
          const rect: SearchRect = {
            x: r.x + (r.w * at) / len,
            y: r.y,
            w: Math.max(1, (r.w * qlen) / len),
            h: r.h,
          };
          pageMatches.push(rect);
          matches.push({
            page,
            index: ord,
            text: snippetText(item.str, q, at),
            ...rect,
          });
          ord += 1;
        }
      }
      if (pageMatches.length) highlightsByPage.set(page, pageMatches);
      pg.cleanup();
    } catch (_) {
      /* skip unreadable page */
    }
  }

  setActiveMatchValue(null);
  refreshHighlights();
  try {
    if (pdf) await pdf.cleanup();
  } catch (_) {
    /* advisory */
  }
  return { ok: true, query, total: matches.length, matches };
}

export function setHighlightMode(mode: "live" | "stale"): void {
  setHighlightModeValue(mode === "stale" ? "stale" : "live");
  const stale = mode === "stale";
  for (const st of stateByCanvasId.values()) {
    if (st.textLayerEl) st.textLayerEl.classList.toggle("search-stale", stale);
  }
}

export function setActiveMatch(page: number, index: number): void {
  const next =
    Number.isFinite(page) && page > 0 && Number.isFinite(index) && index >= 0
      ? { page, index: index | 0 }
      : null;
  setActiveMatchValue(next);
  for (const st of stateByCanvasId.values()) {
    if (!st.textLayerEl) continue;
    const wanted = next && next.page === st.page ? String(next.index) : null;
    for (const d of st.textLayerEl.querySelectorAll(".highlight") as NodeListOf<HTMLElement>) {
      d.classList.toggle("is-active", wanted !== null && d.dataset.match === wanted);
    }
  }
}

export function clearHighlights(): void {
  highlightsByPage.clear();
  setSearchQuery("");
  setActiveMatchValue(null);
  setHighlightModeValue("live");
  for (const st of stateByCanvasId.values()) {
    if (st.host) {
      st.host.querySelectorAll(".highlight").forEach((n) => n.remove());
    }
    if (st.textLayerEl) st.textLayerEl.classList.remove("search-stale");
  }
}


