// Search-match highlight painting, split out of renderer.ts.
//
// Two halves, on purpose. `occurrences` is the pure scan — which characters of
// a span's text the query covers — and `applyHighlights` is the DOM work: a
// Range per occurrence, a box per rect the Range reports. The Rust side of
// search is split the same way (`reader_core::search::occurrence_spans`, and
// the reflowable layer that paints its spans) because the scan's rules are the
// part that has to agree across both pipelines: an occurrence ordinal is how a
// painted box and a row in the results list recognise each other.

import type { PageState } from "./types";
import { session } from "./state";

/** Boxes one page will paint. The reflowable layer keeps the same number for a
 *  row (`MAX_BOXES_PER_ROW` in src/components/formats/reflow/highlight.rs), so
 *  a one-character query in a long paragraph costs the same whichever family
 *  the document is in. */
const MAX_HIGHLIGHTS_PER_PAGE = 200;

/** One occurrence's offsets in a span's text, in UTF-16 code units — the unit
 *  `Range.setStart` counts in, and the unit `String.indexOf` reports. */
export type Occurrence = { start: number; end: number };

/** Every occurrence of `query` in `text`, and whether those offsets may be
 *  handed to a Range over the RAW text node.
 *
 * `query` arrives folded and trimmed (`setSearchContext` does both), matching
 * case-insensitively and advancing by the query's length, so `"aa"` in `"aaa"`
 * is one occurrence — the rules the Rust scan keeps, and the ones the ordinal
 * numbering depends on.
 *
 * The offsets are counted in the FOLDED copy, which is also what the scan of a
 * page's extracted text runs over. They transfer to the raw text only while
 * folding preserves length, and `toLowerCase` does not always: 'İ' is one code
 * unit and folds to two, so every offset after it would point one character
 * early — a box over text nobody searched for. `offsetsUsable` is false in that
 * case, and the caller counts the ordinals without painting, which is the same
 * call the Rust scan makes ("a missed hit is a smaller lie").
 */
export function occurrences(
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

export function applyHighlights(st: PageState): void {
  const { host, textLayerEl } = st;
  if (!host) return;
  host.querySelectorAll(".highlight").forEach((n) => n.remove());
  const query = session.searchQuery;
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
    // A span whose text is not one addressable text node cannot be boxed, but
    // its occurrences still consume ordinals: the numbering has to match the
    // index's, which counts every occurrence in the page whether or not a box
    // landed on it.
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
  const activeOrd =
    session.activeMatch && session.activeMatch.page === st.page ? session.activeMatch.index : -1;
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

export function refreshHighlights(): void {
  for (const st of session.stateByCanvasId.values()) {
    if (st.textLayerEl) applyHighlights(st);
  }
}
