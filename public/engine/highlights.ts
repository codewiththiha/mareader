// Search-match highlight painting: the pure scan and the DOM work.

import type { PageState } from "./types";
import type { EngineSession } from "./state";

// Boxes one page will paint, shared with the reflowable layer.
const MAX_HIGHLIGHTS_PER_PAGE = 200;

// One occurrence's offsets in a span's text, in UTF-16 units.
type Occurrence = { start: number; end: number };

// Every occurrence of `query` in `text`, in the folded copy.
function occurrences(
  text: string,
  query: string,
): { spans: Occurrence[]; offsetsUsable: boolean } {
  const hay = text.toLowerCase();
  const offsetsUsable = hay.length === text.length;
  const spans: Occurrence[] = [];
  if (!query) return { spans, offsetsUsable };
  for (let at = hay.indexOf(query); at !== -1; at = hay.indexOf(query, at + query.length)) {
    spans.push({ start: at, end: at + query.length });
  }
  return { spans, offsetsUsable };
}

// The ordinal this page emphasises, or -1.
function activeOrdinal(s: EngineSession, page: number): number {
  const active = s.activeMatch;
  return active && active.page === page ? active.index : -1;
}

/** Drop every highlight box on a page, leaving the text layer itself alone. */
export function clearHighlightBoxes(st: PageState): void {
  st.host?.querySelectorAll(".highlight").forEach((n) => n.remove());
}

/** Re-mark which painted box is the active match, without repainting any. */
export function markActiveHighlight(s: EngineSession, st: PageState): void {
  if (!st.textLayerEl) return;
  const ord = activeOrdinal(s, st.page);
  const wanted = ord >= 0 ? String(ord) : null;
  for (const d of st.textLayerEl.querySelectorAll(".highlight") as NodeListOf<HTMLElement>) {
    d.classList.toggle("is-active", wanted !== null && d.dataset.match === wanted);
  }
}

export function applyHighlights(s: EngineSession, st: PageState): void {
  const { host, textLayerEl } = st;
  if (!host) return;
  clearHighlightBoxes(st);
  const query = s.searchQuery;
  if (!query || !textLayerEl) return;
  const origin = host.getBoundingClientRect();
  const boxes: { r: DOMRect; ord: number }[] = [];
  let ord = 0;
  for (const span of textLayerEl.querySelectorAll("span")) {
    const text = span.textContent;
    if (!text) continue;
    const node = span.firstChild;
    const textNode = node && node.nodeType === Node.TEXT_NODE ? (node as Text) : null;
    const { spans, offsetsUsable } = occurrences(text, query);
    // A span that is not one text node still consumes ordinals.
    const paintable = !!textNode && textNode.length >= query.length && offsetsUsable;
    for (const { start, end } of spans) {
      const mine = ord;
      ord += 1;
      if (!paintable) continue;
      let rects: DOMRectList | undefined;
      try {
        const range = document.createRange();
        range.setStart(textNode, start);
        range.setEnd(textNode, end);
        rects = range.getClientRects();
        range.detach?.();
      } catch (_) {
        continue;
      }
      if (!rects) continue;
      for (const r of rects) {
        if (r.width <= 0 || r.height <= 0) continue;
        boxes.push({ r, ord: mine });
        if (boxes.length >= MAX_HIGHLIGHTS_PER_PAGE) break;
      }
      if (boxes.length >= MAX_HIGHLIGHTS_PER_PAGE) break;
    }
    if (boxes.length >= MAX_HIGHLIGHTS_PER_PAGE) break;
  }
  const activeOrd = activeOrdinal(s, st.page);
  for (const { r, ord: n } of boxes) {
    const d = document.createElement("div");
    d.className = n === activeOrd ? "highlight is-active" : "highlight";
    d.dataset.match = String(n);
    d.style.left = r.x - origin.x + "px";
    d.style.top = r.y - origin.y + "px";
    d.style.width = Math.max(1, r.width) + "px";
    d.style.height = Math.max(1, r.height) + "px";
    textLayerEl.appendChild(d);
  }
}

export function refreshHighlights(s: EngineSession): void {
  for (const st of s.stateByCanvasId.values()) {
    if (st.textLayerEl) applyHighlights(s, st);
  }
}
