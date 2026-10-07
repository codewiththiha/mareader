// Selection page-range tracking and the AI pill's rich detail.

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

// Detail is debounced and deduped on text+position.
let detailDebounce: ReturnType<typeof setTimeout> | null = null;
let lastDetailKey: string | null = null;
// Plain clicks produce no selectionchange, so a press schedules a recheck.
let clickClearTimer: ReturnType<typeof setTimeout> | null = null;
// Set on every mousedown: true when the press landed inside the AI UI.
let pointerDownInAiUi = false;

// Hosts advertise their family and page via the dom-contract.

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
  // A host that does not declare its page carries it in its id.
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

// A reflowable document has no page grid: the mark keeps block offsets.
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

  // Offsets count characters (code points) of the block's rendered text.
  const full = row.textContent ?? "";
  if (!full) return null;
  const before = range.cloneRange();
  before.selectNodeContents(row);
  try {
    before.setEnd(range.startContainer, range.startOffset);
  } catch {
    // A start the row does not contain: no honest spot.
    return null;
  }
  const start = [...before.toString()].length;
  const end = start + [...range.toString()].length;
  const total = [...full].length;
  if (end <= start || start >= total) return null;
  return { block, start, end: Math.min(end, total) };
}

// Events are raised on the page host; a clear goes to the window.
function raise(origin: Element | null, name: string, detail: unknown): void {
  const event = new CustomEvent(name, { detail, bubbles: true });
  if (origin && origin.isConnected) origin.dispatchEvent(event);
  else globalThis.dispatchEvent(event);
}

function dispatchSelectionPages(): void {
  const sel = document.getSelection();
  if (!sel || sel.rangeCount === 0 || sel.isCollapsed) {
    if (!selDragging && lastSelectionRangeKey !== null) {
      lastSelectionRangeKey = null;
      lastKnownAnchorPage = null;
      lastKnownFocusPage = null;
      raise(null, SELECTION_PAGES_EVENT, null);
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
  raise(hostOf(sel.anchorNode) ?? hostOf(sel.focusNode), SELECTION_PAGES_EVENT, { first, last });
}

// ~120 chars of surrounding text from the selection's own layer.
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
    raise(null, SELECTION_DETAIL_EVENT, null);
    return;
  }

  const range = sel.getRangeAt(0);
  // getBoundingClientRect() is the tight box — the warp-window anchor.
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
  // The spot is only computed for a reflowable document.
  const spot = kind === HOST_REFLOW ? findReflowSpot(range) : null;

  raise(host, SELECTION_DETAIL_EVENT, {
    text,
    context: extractContext(range, text),
    rect: {
      x: rect.left,
      y: rect.top,
      width: rect.width,
      height: rect.height,
    },
    // Which family the selection is in; null when neither.
    host: kind,
    spot,
  });
}

export function installSelectionTracker(): void {
  document.addEventListener("mousedown", (e) => {
    const t = e.target as HTMLElement | null;
    // A drag coalesces the detail pass onto mouseup.
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
    // During a drag, coalesce onto mouseup; debounce only for keyboard.
    if (selDragging) return;
    if (detailDebounce) clearTimeout(detailDebounce);
    detailDebounce = setTimeout(dispatchSelectionDetail, 120);
  });
}
