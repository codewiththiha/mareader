// The engine's half of the window-event protocol.
//
// The engine talks to the app by dispatching CustomEvents on `window`: it
// owns the PDF.js side and cannot hold a Leptos signal, and the app owns the
// reactive side and cannot be called from a bundled IIFE. So the boundary is
// three event names, and a name that disagrees on the two sides fails at
// runtime and nowhere else — no compiler, no type, no error. A misspelled
// constant here does not throw; it silently stops internal links from
// navigating, or the "Explain" pill from ever appearing over a selection.
//
// The app's full table is `src/events.rs`, which also holds the events the app
// dispatches to itself (`pdfreader:gloss-open`, `pdfreader:ai-chunk`, ...).
// Only these three cross the boundary in this direction, so only these three
// are declared here. `tools/check-events.ts` fails CI when the two tables
// disagree, or when a raw `pdfreader:` literal appears anywhere but the tables.

/** Internal link jump: the engine's link layer asks the app to turn to a page. */
export const NAVIGATE_EVENT = "pdfreader:navigate";

/** Page-range selection, for virtualization pinning (detail may be null). */
export const SELECTION_PAGES_EVENT = "pdfreader:selection-pages";

/** Text-selection detail — word, sentence, rect, host, spot — for the AI pill. */
export const SELECTION_DETAIL_EVENT = "pdfreader:selection-detail";
