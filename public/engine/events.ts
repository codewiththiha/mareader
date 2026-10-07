// The engine's half of the window-event protocol: three names, no
// compiler.

// Internal link jump: the engine asks the app to turn to a page.
export const NAVIGATE_EVENT = "mareader:navigate";

/** Page-range selection, for virtualization pinning (detail may be null). */
export const SELECTION_PAGES_EVENT = "mareader:selection-pages";

// Text-selection detail for the AI pill.
export const SELECTION_DETAIL_EVENT = "mareader:selection-detail";
