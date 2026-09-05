// Selection page-range tracking for virtualization pinning, plus the rich
// selection detail (text / context / bounding rect) the AI explain feature
// anchors its floating pill to.
// See the original pdfEngine.ts commentary: no clamp mid-drag, preserve
// last-known pages across inter-page gaps.
//
// Lives outside `public/engine/` because nothing here is the PDF engine's:
// it reads the host protocol from `../engine/dom-contract` and answers for
// every format that tags a page host. Bundled on its own (see
// `public/readerEngine.ts`) so the reader layer is not carried in by the
// bundle that also drags pdf.js along.

import {
  AI_POPOVER_SELECTOR,
  BLOCK_INDEX_ATTR,
  BLOCK_ROW_SELECTOR,
  HOST_ATTR,
  HOST_PAGE_ATTR,
  HOST_REFLOW,
  HOST_SELECTOR,
  pageFromHostId,
  pageFromWrapId,
  STREAM_WRAP_SELECTOR,
  TEXT_LAYER_SELECTOR,
} from "../engine/dom-contract";
import { SELECTION_DETAIL_EVENT, SELECTION_PAGES_EVENT } from "../engine/events";

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
// recheck, so a stale detail (ghost "Explain" pill) cannot linger.
let clickClearTimer: ReturnType<typeof setTimeout> | null = null;
// Set by every mousedown: true when the press landed inside the AI UI
// (the Explain pill and its popover, marked [data-ai-popover]). Pressing the
// "Explain" button collapses the document selection, but that collapse must
// NOT clear the detail state — the button click fires right after and still needs the
// detail (and its anchor rect) to be there. Kept until the next mousedown
// rather than cleared on mouseup: the debounced clear runs after mouseup.
let pointerDownInAiUi = false;

// Every reader page host advertises itself with the format family that painted
// it and the 1-based page it is showing (both in `./dom-contract`). Asking for
// those two instead of for `.pdf-page` is what lets a selection inside a page of
// type be a selection like any other: no selector here grows a second class when
// a format arrives.

function hostOf(node: Node | null): Element | null {
  if (!node) return null;
  const el = node.nodeType === Node.TEXT_NODE
    ? node.parentElement
    : (node as Element | null);
  return el ? el.closest(HOST_SELECTOR) : null;
}

function findPageNumber(node: Node | null): number | null {
  const host = hostOf(node);
  if (!host) return null;
  const declared = host.getAttribute(HOST_PAGE_ATTR);
  if (declared) {
    const page = parseInt(declared, 10);
    if (Number.isFinite(page) && page > 0) return page;
  }
  // A host that does not declare its page still carries it in its id, and so
  // do the wrappers around a strip's pages (a selection in the gap between two
  // pages lands on a wrapper). Kept as the fallback it has always been.
  if (host.id) {
    const fromId = pageFromHostId(host.id);
    if (fromId !== null) return fromId;
  }
  const el = node && (node.nodeType === Node.TEXT_NODE ? node.parentElement : (node as Element));
  const wrapEl = el ? el.closest(STREAM_WRAP_SELECTOR) : null;
  if (wrapEl && wrapEl.id) {
    const fromWrap = pageFromWrapId(wrapEl.id);
    if (fromWrap !== null) return fromWrap;
  }
  return null;
}

// A reflowable document has no fixed page grid, so a page-space rect cannot be
// what a gloss mark remembers there: a font-size change, a window resize or the
// measure pass settling all re-cut the pages and the rect drifts onto other
// words. What survives every re-flow is the BLOCK the words sit in and how far
// into that block's rendered text they start, so that is what this reports and
// what the app persists (see `components/ai/reflow_anchor.rs`).
type ReflowSpot = { block: number; start: number; end: number };

function findReflowSpot(range: Range): ReflowSpot | null {
  const startEl = range.startContainer.nodeType === Node.TEXT_NODE
    ? range.startContainer.parentElement
    : (range.startContainer as Element | null);
  if (!startEl) return null;
  const row = startEl.closest(BLOCK_ROW_SELECTOR);
  if (!row) return null;
  const rawBlock = row.getAttribute(BLOCK_INDEX_ATTR);
  if (!rawBlock) return null;
  const block = parseInt(rawBlock, 10);
  if (!Number.isFinite(block) || block < 0) return null;

  // Offsets count CHARACTERS (Unicode code points) of the block's rendered
  // text, in the text nodes under the row in document order — the same
  // coordinate system the app walks when it projects the spot back to pixels
  // (`components/ai/reflow_anchor.rs`). Counting code points rather than UTF-16
  // units is what keeps an emoji or a mathematical alphanumeric ONE character
  // on both sides of the wire.
  //
  // `Range.toString()` is defined as the concatenation of the Text nodes the
  // range partially contains, in tree order, with no filtering — so a range
  // from the row's start to the selection's start counts exactly the characters
  // before it, for an element container as readily as for a text one.
  const full = row.textContent ?? "";
  if (!full) return null;
  const before = range.cloneRange();
  before.selectNodeContents(row);
  try {
    before.setEnd(range.startContainer, range.startOffset);
  } catch {
    // A start the row does not contain (a selection re-anchored mid-flight):
    // no honest spot to report, and the app falls back to its own capture.
    return null;
  }
  const start = [...before.toString()].length;
  const end = start + [...range.toString()].length;
  const total = [...full].length;
  if (end <= start || start >= total) return null;
  return { block, start, end: Math.min(end, total) };
}

function dispatchSelectionPages(): void {
  const sel = document.getSelection();
  if (!sel || sel.rangeCount === 0 || sel.isCollapsed) {
    if (!selDragging && lastSelectionRangeKey !== null) {
      lastSelectionRangeKey = null;
      lastKnownAnchorPage = null;
      lastKnownFocusPage = null;
      globalThis.dispatchEvent(
        new CustomEvent(SELECTION_PAGES_EVENT, { detail: null })
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
    new CustomEvent(SELECTION_PAGES_EVENT, {
      detail: { first, last },
    })
  );
}

// ~120 chars of surrounding text from the same layer of the document, giving
// the model enough context to disambiguate the selected word. The layer is a
// PDF's text layer or a reflowable block's content column — whichever the
// selection is actually in, so the context is the reader's own words and not
// the whole page.
// The text a sentence of context is cut out of, for whichever format painted
// the selection. Nothing here names a format's classes: a PDF's text layer and
// a reflowable document's BLOCK ROW are both "the element this selection is
// inside", found by the attributes the hosts publish.
//
// Scoping a reflowable selection to its block row rather than to the page is
// also the better sentence: a page of type is thousands of characters, and the
// model disambiguates a word from the clause around it, not from the chapter.
// A selection that starts in one block and ends in another still gets its
// start's row, which is the row its spot counts characters in.
function contextLayer(node: Node | null): Element | null {
  const el = node && (node.nodeType === Node.TEXT_NODE
    ? node.parentElement
    : (node as Element | null));
  if (!el) return null;
  return el.closest(BLOCK_ROW_SELECTOR) ?? el.closest(TEXT_LAYER_SELECTOR);
}

function extractContext(range: Range, selectedText: string): string {
  const layer = contextLayer(range.startContainer);
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
      new CustomEvent(SELECTION_DETAIL_EVENT, { detail: null })
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

  const host = hostOf(range.startContainer);
  const kind = host ? host.getAttribute(HOST_ATTR) : null;
  // The spot is only meaningful — and only computed — for a reflowable
  // document; a PDF's anchor is the page-space rect the app derives itself.
  const spot = kind === HOST_REFLOW ? findReflowSpot(range) : null;

  globalThis.dispatchEvent(
    new CustomEvent(SELECTION_DETAIL_EVENT, {
      detail: {
        text,
        context: extractContext(range, text),
        rect: {
          x: rect.left,
          y: rect.top,
          width: rect.width,
          height: rect.height,
        },
        // Which format family the selection is in. Absent (null) when it is in
        // neither — chrome, the library — and the app then treats it as the
        // PDF path it has always been.
        host: kind,
        spot,
      },
    })
  );
}

export function installSelectionTracker(): void {
  document.addEventListener("mousedown", (e) => {
    const t = e.target as HTMLElement | null;
    // A drag inside any reader host (a PDF's text layer, a page of type, the
    // continuous stream) coalesces the detail pass onto mouseup: per-move
    // `getClientRects()` is a layout read, and a paragraph of text is worth
    // exactly as much protection as a page of pixels.
    if (t && t.closest && t.closest(HOST_SELECTOR)) {
      selDragging = true;
    }
    pointerDownInAiUi = !!(t && t.closest && t.closest(AI_POPOVER_SELECTOR));
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
