// The engine's half of the DOM contract.
//
// The app builds the hosts; the engine paints into them. The two sides never
// call each other — they meet in the DOM, through attribute names, the values
// those attributes carry, two class names, and the shape of element ids. A
// name that disagrees is not an error anywhere: it is a `closest` that returns
// null, so a selection inside a page stops producing an "Explain" pill, or a
// canvas stops finding the host it was registered against, and the only symptom
// is a reader that quietly does nothing.
//
// The app's half is `src/dom_contract.rs`, which holds the names Rust uses as
// values. Two of them — `data-host-page` and `data-ai-popover` — cannot live
// there, because a Leptos view takes an attribute's name from the markup and
// only its value from an expression, so the hosts write them as literals; the
// check below reads those literals out of the Rust source instead.
//
// `tools/check-dom-contract.ts` fails CI when the halves disagree. It also
// reads the id shapes out of the Rust builders' `format!` strings rather than
// trusting a copy of them, so a rename on either side is caught by the other.
// Nothing outside this file should spell any of these names.

// --- attributes every reader host carries ---------------------------------

/** Names the format family that painted a host: `pdf` or `reflow`. */
export const HOST_ATTR = "data-reader-host";

/** The 1-based page a host is showing. The id shape is only the fallback. */
export const HOST_PAGE_ATTR = "data-host-page";

/** On a rendered block of a reflowable document: its index in document order. */
export const BLOCK_INDEX_ATTR = "data-block-index";

/** On the AI pill's root: a press here is not a click that clears a selection. */
export const AI_POPOVER_ATTR = "data-ai-popover";

// --- values `HOST_ATTR` carries -------------------------------------------

/**
 * The engine only ever branches on `reflow`: a PDF host is the path it has
 * always taken, and the app decides what to do with a `host` value it does not
 * recognise. `HOST_PDF` is therefore the app's to declare (`src/dom_contract.rs`),
 * and the check forbids spelling either value as a literal all the same.
 */
export const HOST_REFLOW = "reflow";

// --- class names both sides look up ---------------------------------------

/** The text layer inside a PDF host: the app builds it, the engine fills it. */
export const TEXT_LAYER_CLASS = "textLayer";

/** The still-bitmap overlay a zoom stretches while a re-render is on its way. */
export const PAGE_SNAPSHOT_CLASS = "page-snapshot";

// --- element id shapes ----------------------------------------------------
//
// `components/viewer/page_host.rs` builds these. Three of the four prefixes
// number their pages from 1; the continuous strip indexes its window from 0,
// and so do its ids, which is the one asymmetry a caller has to know.

export const ID_PREFIX_SINGLE = "sp";
export const ID_PREFIX_SPREAD = "dp";
export const ID_PREFIX_HSCROLL = "hp";
export const ID_PREFIX_STREAM = "cont";

/** Suffix of a page host's id. */
export const HOST_ID_SUFFIX = "-pg";
/** Suffix of the canvas inside a host: the host id with this instead. */
export const CANVAS_ID_SUFFIX = "-cv";
/** Suffix of the wrapper row around one page of a vertical strip. */
export const STREAM_WRAP_SUFFIX = "-wrap";

// --- derived selectors ----------------------------------------------------

export const HOST_SELECTOR = `[${HOST_ATTR}]`;
export const BLOCK_ROW_SELECTOR = `[${BLOCK_INDEX_ATTR}]`;
export const AI_POPOVER_SELECTOR = `[${AI_POPOVER_ATTR}]`;
export const TEXT_LAYER_SELECTOR = `.${TEXT_LAYER_CLASS}`;
export const PAGE_SNAPSHOT_SELECTOR = `.${PAGE_SNAPSHOT_CLASS}`;
/** A selection in the gap between two pages of the strip lands on a wrapper. */
export const STREAM_WRAP_SELECTOR = `[id^='${ID_PREFIX_STREAM}-'][id$='${STREAM_WRAP_SUFFIX}']`;

// --- id parsing -----------------------------------------------------------
//
// Compiled once, not per call: `pageFromCanvasId` runs on the fallback path of
// every unmounted-id resolution, which on a fast scroll is per row.

const PREFIX_GROUP = [ID_PREFIX_SINGLE, ID_PREFIX_SPREAD, ID_PREFIX_HSCROLL, ID_PREFIX_STREAM].join(
  "|"
);
const HOST_PAGE_RE = new RegExp(`^(${PREFIX_GROUP})-(\\d+)${HOST_ID_SUFFIX}$`);
const CANVAS_PAGE_RE = new RegExp(`^(${PREFIX_GROUP})-(\\d+)${CANVAS_ID_SUFFIX}$`);
const STREAM_WRAP_RE = new RegExp(`^${ID_PREFIX_STREAM}-(\\d+)${STREAM_WRAP_SUFFIX}$`);

/** 1-based page from a `^(prefix)-(n)$` match; the strip's ids are 0-based. */
function pageOf(prefix: string | undefined, raw: string | undefined): number | null {
  if (!raw) return null;
  const n = parseInt(raw, 10);
  if (!Number.isFinite(n) || n < 0) return null;
  return prefix === ID_PREFIX_STREAM ? n + 1 : n;
}

/** The 1-based page a host id names, or null when the id is not a page host's. */
export function pageFromHostId(id: string): number | null {
  const m = HOST_PAGE_RE.exec(id);
  return m ? pageOf(m[1], m[2]) : null;
}

/** The 1-based page a canvas id names, or null when the id is not a canvas's. */
export function pageFromCanvasId(id: string): number | null {
  const m = CANVAS_PAGE_RE.exec(id);
  return m ? pageOf(m[1], m[2]) : null;
}

/** The 1-based page a strip wrapper's id names, or null for any other id. */
export function pageFromWrapId(id: string): number | null {
  const m = STREAM_WRAP_RE.exec(id);
  return m ? pageOf(ID_PREFIX_STREAM, m[1]) : null;
}

/** The host a canvas id belongs to: the same id with the host suffix. */
export function hostIdFromCanvasId(canvasId: string): string {
  if (!canvasId.endsWith(CANVAS_ID_SUFFIX)) return canvasId;
  return canvasId.slice(0, -CANVAS_ID_SUFFIX.length) + HOST_ID_SUFFIX;
}
