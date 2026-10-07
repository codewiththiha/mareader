// The engine's half of the DOM contract: the names the app writes.

/** Names the format family that painted a host: `pdf` or `reflow`. */
export const HOST_ATTR = "data-reader-host";

/** The 1-based page a host is showing. The id shape is only the fallback. */
export const HOST_PAGE_ATTR = "data-host-page";

// On a rendered block: its index in document order.
export const BLOCK_INDEX_ATTR = "data-block-index";

// On the AI pill's root: a press here is not a clearing click.
export const AI_POPOVER_ATTR = "data-ai-popover";

// The session a thumbnail canvas belongs to (its sid).
export const SESSION_ATTR = "data-engine-sid";

// The engine only branches on `reflow`; the app declares `HOST_PDF`.
export const HOST_REFLOW = "reflow";

/** The text layer inside a PDF host: the app builds it, the engine fills it. */
export const TEXT_LAYER_CLASS = "textLayer";

// The still-bitmap overlay a zoom stretches.
export const PAGE_SNAPSHOT_CLASS = "page-snapshot";

// Four id prefixes; the continuous strip indexes from 0, the rest from 1.

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

export const HOST_SELECTOR = `[${HOST_ATTR}]`;
export const BLOCK_ROW_SELECTOR = `[${BLOCK_INDEX_ATTR}]`;
export const AI_POPOVER_SELECTOR = `[${AI_POPOVER_ATTR}]`;
export const TEXT_LAYER_SELECTOR = `.${TEXT_LAYER_CLASS}`;
export const PAGE_SNAPSHOT_SELECTOR = `.${PAGE_SNAPSHOT_CLASS}`;
/** A selection in the gap between two pages of the strip lands on a wrapper. */
export const STREAM_WRAP_SELECTOR = `[id^='${ID_PREFIX_STREAM}-'][id$='${STREAM_WRAP_SUFFIX}']`;

// Compiled once: `pageFromCanvasId` runs per row on a fast scroll.

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

// The 1-based page a host id names, or null.
export function pageFromHostId(id: string): number | null {
  const m = HOST_PAGE_RE.exec(id);
  return m ? pageOf(m[1], m[2]) : null;
}

// The 1-based page a canvas id names, or null.
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
