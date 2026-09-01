// Search-match highlight painting, split out of renderer.ts.

import type { PageState } from "./types";
import { session } from "./state";

export function applyHighlights(st: PageState): void {
  const { host, textLayerEl } = st;
  if (!host) return;
  host.querySelectorAll(".highlight").forEach((n) => n.remove());
  if (!session.searchQuery || !textLayerEl) return;
  const origin = host.getBoundingClientRect();
  const boxes: { r: DOMRect; ord: number }[] = [];
  const qlen = session.searchQuery.length;
  let ord = 0;
  for (const span of textLayerEl.querySelectorAll("span")) {
    const text = span.textContent;
    if (!text) continue;
    const hay = text.toLowerCase();
    if (!hay.includes(session.searchQuery)) continue;
    const node = span.firstChild;
    const textNode = node && node.nodeType === Node.TEXT_NODE ? (node as Text) : null;
    const addressable = !!(textNode && textNode.length >= qlen);
    for (
      let at = hay.indexOf(session.searchQuery);
      at !== -1;
      at = hay.indexOf(session.searchQuery, at + qlen)
    ) {
      const mine = ord;
      ord += 1;
      if (!addressable) continue;
      let rects: DOMRectList | undefined;
      try {
        if (!textNode) continue;
        const range = document.createRange();
        range.setStart(textNode, at);
        range.setEnd(textNode, at + qlen);
        rects = range.getClientRects();
        range.detach?.();
      } catch (_) {
        continue;
      }
      if (!rects) continue;
      for (const r of rects) {
        if (r.width <= 0 || r.height <= 0) continue;
        boxes.push({ r, ord: mine });
        if (boxes.length >= 200) break;
      }
      if (boxes.length >= 200) break;
    }
    if (boxes.length >= 200) break;
  }
  const activeOrd =
    session.activeMatch && session.activeMatch.page === st.page ? session.activeMatch.index : -1;
  const MAX_HIGHLIGHTS_PER_PAGE = 200;
  const painted = boxes.slice(0, MAX_HIGHLIGHTS_PER_PAGE);
  for (const { r, ord: n } of painted) {
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
