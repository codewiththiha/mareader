// The engine's half of the window-event protocol. The engine talks to the
// app by CustomEvents the app hears on `window` — raised ON the element they
// came from (the clicked link, the selection's page host) and bubbling, so
// each of several panes keeps only its own; a clear is raised on the window
// itself. The engine owns the pdf.js side and
// cannot hold a Leptos signal, and the app cannot be called from a bundled
// IIFE. So the boundary is three event names, and a name that disagrees
// fails at runtime and nowhere else — no compiler, no type, no error:
// internal links silently stop navigating, or the "Explain" pill never
// appears. The app's full table is `src/events.rs` (which also holds the
// events the app dispatches to itself); only these three cross in this
// direction. `tools/check-events.ts` fails CI when the tables disagree or a
// raw `pdfreader:` or `mareader:` literal appears anywhere but the tables.

/** Internal link jump: the engine's link layer asks the app to turn to a page. */
export const NAVIGATE_EVENT = "mareader:navigate";

/** Page-range selection, for virtualization pinning (detail may be null). */
export const SELECTION_PAGES_EVENT = "mareader:selection-pages";

/** Text-selection detail — word, sentence, rect, host, spot — for the AI pill. */
export const SELECTION_DETAIL_EVENT = "mareader:selection-detail";
