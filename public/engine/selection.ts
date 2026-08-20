// Selection page-range tracking for virtualization pinning.
// See the original pdfEngine.ts commentary: no clamp mid-drag, preserve
// last-known pages across inter-page gaps.

let selDragging = false;
let lastKnownAnchorPage: number | null = null;
let lastKnownFocusPage: number | null = null;
let lastSelectionRangeKey: string | null = null;

function findPageNumber(node: Node | null): number | null {
  if (!node) return null;
  const el = node.nodeType === Node.TEXT_NODE
    ? node.parentElement
    : (node as HTMLElement | null);
  if (!el) return null;
  const pageEl = el.closest(".pdf-page");
  if (pageEl && pageEl.id) {
    const m = /^cont-(\d+)-pg$/.exec(pageEl.id);
    if (m && m[1]) return parseInt(m[1], 10) + 1;
  }
  const wrapEl = el.closest("[id^='cont-'][id$='-wrap']");
  if (wrapEl && wrapEl.id) {
    const m = /^cont-(\d+)-wrap$/.exec(wrapEl.id);
    if (m && m[1]) return parseInt(m[1], 10) + 1;
  }
  return null;
}

function dispatchSelectionPages(): void {
  const sel = document.getSelection();
  if (!sel || sel.rangeCount === 0 || sel.isCollapsed) {
    if (!selDragging && lastSelectionRangeKey !== null) {
      lastSelectionRangeKey = null;
      lastKnownAnchorPage = null;
      lastKnownFocusPage = null;
      globalThis.dispatchEvent(
        new CustomEvent("pdfreader:selection-pages", { detail: null })
      );
    }
    return;
  }

  const anchorPage = findPageNumber(sel.anchorNode) ?? lastKnownAnchorPage;
  const focusPage = findPageNumber(sel.focusNode) ?? lastKnownFocusPage;

  if (anchorPage !== null) lastKnownAnchorPage = anchorPage;
  if (focusPage !== null) lastKnownFocusPage = focusPage;

  if (anchorPage === null || focusPage === null) {
    return;
  }

  const first = Math.min(anchorPage, focusPage);
  const last = Math.max(anchorPage, focusPage);
  const key = `${first}-${last}`;
  if (key === lastSelectionRangeKey) return;
  lastSelectionRangeKey = key;
  globalThis.dispatchEvent(
    new CustomEvent("pdfreader:selection-pages", {
      detail: { first, last },
    })
  );
}

export function installSelectionTracker(): void {
  document.addEventListener("mousedown", (e) => {
    const t = e.target as HTMLElement | null;
    if (t && t.closest && t.closest(".textLayer")) {
      selDragging = true;
    }
  });

  window.addEventListener("mouseup", () => {
    selDragging = false;
    dispatchSelectionPages();
  });

  document.addEventListener("selectionchange", () => {
    dispatchSelectionPages();
  });
}
