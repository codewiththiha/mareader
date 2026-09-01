// Selection page-range tracking for virtualization pinning, plus the rich
// selection detail (text / context / bounding rect) the AI explain feature
// anchors its floating menu to.
// See the original pdfEngine.ts commentary: no clamp mid-drag, preserve
// last-known pages across inter-page gaps.

let selDragging = false;
let lastKnownAnchorPage: number | null = null;
let lastKnownFocusPage: number | null = null;
let lastSelectionRangeKey: string | null = null;

// Detail side: debounced so a drag doesn't fire one event per selectionchange,
// and deduped on text+position so the Rust side only sees real transitions
// (a `.set()` there always notifies, even on unchanged values).
let detailDebounce: ReturnType<typeof setTimeout> | null = null;
let lastDetailKey: string | null = null;
// Plain clicks (no drag) produce NO selectionchange when the selection is
// already collapsed — exactly the state the AI UI leaves behind after it
// suppresses a clear. Any press outside the AI UI therefore schedules one
// recheck, so a stale detail (ghost "Info" pill) cannot linger.
let clickClearTimer: ReturnType<typeof setTimeout> | null = null;
// Set by every mousedown: true when the press landed inside the AI UI
// (selection menu / popover, marked [data-ai-popover]). Pressing the "Info"
// button collapses the document selection, but that collapse must NOT clear
// the detail state — the button click fires right after and still needs the
// detail (and its anchor rect) to be there. Kept until the next mousedown
// rather than cleared on mouseup: the debounced clear runs after mouseup.
let pointerDownInAiUi = false;

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

// ~120 chars of surrounding text from the same text layer, giving the model
// enough context to disambiguate the selected word.
function extractContext(range: Range, selectedText: string): string {
  const startEl = range.startContainer.nodeType === Node.TEXT_NODE
    ? range.startContainer.parentElement
    : (range.startContainer as HTMLElement | null);
  const layer = startEl ? startEl.closest(".textLayer") : null;
  if (!layer) return selectedText;
  const fullText = layer.textContent ?? "";
  const idx = fullText.indexOf(selectedText);
  if (idx === -1) return selectedText;
  const start = Math.max(0, idx - 60);
  const end = Math.min(fullText.length, idx + selectedText.length + 60);
  return fullText.slice(start, end).trim();
}

function dispatchSelectionDetail(): void {
  const sel = document.getSelection();
  const text = sel && sel.rangeCount > 0 && !sel.isCollapsed
    ? sel.toString().trim()
    : "";

  if (!text || !sel || sel.rangeCount === 0) {
    // A collapse caused by pressing inside the AI UI is not a real clear.
    if (pointerDownInAiUi) return;
    // Dedupe consecutive clears: only genuine transitions reach the app.
    if (lastDetailKey === null) return;
    lastDetailKey = null;
    globalThis.dispatchEvent(
      new CustomEvent("pdfreader:selection-detail", { detail: null })
    );
    return;
  }

  const range = sel.getRangeAt(0);
  // getBoundingClientRect() is the tight box around all selected fragments —
  // the "warp window" anchor. Degenerate ranges (zero-size) fall back to the
  // first client rect, which covers multi-line selections.
  const bounds = range.getBoundingClientRect();
  const rect = bounds.width > 0 && bounds.height > 0
    ? bounds
    : range.getClientRects()[0];
  if (!rect) return;

  const key = `${text}@${Math.round(rect.left)},${Math.round(rect.top)}:${Math.round(rect.width)}x${Math.round(rect.height)}`;
  if (key === lastDetailKey) return;
  lastDetailKey = key;

  globalThis.dispatchEvent(
    new CustomEvent("pdfreader:selection-detail", {
      detail: {
        text,
        context: extractContext(range, text),
        rect: {
          x: rect.left,
          y: rect.top,
          width: rect.width,
          height: rect.height,
        },
      },
    })
  );
}

export function installSelectionTracker(): void {
  document.addEventListener("mousedown", (e) => {
    const t = e.target as HTMLElement | null;
    if (t && t.closest && t.closest(".textLayer")) {
      selDragging = true;
    }
    pointerDownInAiUi = !!(t && t.closest && t.closest("[data-ai-popover]"));
    if (!pointerDownInAiUi) {
      if (clickClearTimer) clearTimeout(clickClearTimer);
      clickClearTimer = setTimeout(dispatchSelectionDetail, 200);
    }
  });

  window.addEventListener("mouseup", () => {
    selDragging = false;
    dispatchSelectionPages();
    // The detail pass the drag coalesced: run it once, debounced, exactly
    // like a keyboard selection's.
    if (detailDebounce) clearTimeout(detailDebounce);
    detailDebounce = setTimeout(dispatchSelectionDetail, 120);
  });

  document.addEventListener("selectionchange", () => {
    dispatchSelectionPages();
    // While a drag is in progress, selectionchange fires per mouse move and
    // getClientRects() is a layout read. Coalesce the detail pass onto the
    // drag's end (mouseup) and only debounce here for keyboard selection.
    if (selDragging) return;
    if (detailDebounce) clearTimeout(detailDebounce);
    detailDebounce = setTimeout(dispatchSelectionDetail, 120);
  });
}
