// The shelf/reader handoff, as data. No DOM. The boot script and the web-lane
// test both consume this module, so a decision change has one place to land.
//
// The storage key uses a dot. A colon would be a window-event literal, and
// the event-name check fails those outside the two event tables. This is not
// an event.

export const HANDOFF_KEY = "mareader.handoff";

export type SessionKind = "library" | "reader";
export type HistoryMode = "push" | "replace" | "reload";

export interface Handoff {
  path: string;
  bookId?: string;
}

export function parseHandoff(raw: string | null): Handoff | null {
  if (!raw) return null;
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof value !== "object" || value === null) return null;
  const record = value as { path?: unknown; bookId?: unknown };
  if (typeof record.path !== "string" || record.path.length === 0) return null;
  if (typeof record.bookId === "string" && record.bookId.length > 0) {
    return { path: record.path, bookId: record.bookId };
  }
  return { path: record.path };
}

/** Reader only when the query asks for it AND a handoff is waiting. A reader
 *  URL with nothing stored is a shelf boot — the page that wrote the query
 *  died before it could write the payload, or the payload was already taken. */
export function decideSession(search: string, raw: string | null): SessionKind {
  const params = new URLSearchParams(search.startsWith("?") ? search.slice(1) : search);
  if (params.get("session") !== "reader") return "library";
  return parseHandoff(raw) ? "reader" : "library";
}

/** The entry URL with the session query set or cleared. Origin and path stay:
 *  a path of `/reader` is a different GET, and Tauri will not SPA-fallback it. */
export function withSession(current: string, kind: SessionKind): string {
  const url = new URL(current);
  if (kind === "reader") url.searchParams.set("session", "reader");
  else url.searchParams.delete("session");
  return url.href;
}

/** Same document means the navigation would not load anything. The caller
 *  reloads instead, which is how a second open recycles a reader that is
 *  already at `?session=reader`. */
export function navigation(
  current: string,
  next: string,
  prefer: "push" | "replace",
): HistoryMode {
  if (new URL(current).href === new URL(next).href) return "reload";
  return prefer;
}
