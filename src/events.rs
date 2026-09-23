//! The app's window-`CustomEvent` protocol: every event name in one table,
//! plus the typed dispatchers.
//!
//! Window CustomEvents are the app's cross-cutting message mechanism — they
//! cross layer boundaries (services → components, engine JS → Rust) without
//! either side holding a signal from the other. Three of these names are also
//! a protocol with the imperative engine, which dispatches them from plain JS
//! and declares them in `public/engine/events.ts`. `tools/check-events.ts`
//! fails CI when the two tables disagree or when a name is spelled as a
//! literal elsewhere — a mismatch is not a compile error on either side, only
//! a dispatch into a window nobody is listening on.

use serde::Serialize;

/// AI chunk stream, bridged from the Tauri backend by `services::ai`.
pub const AI_CHUNK_EVENT: &str = "mareader:ai-chunk";
/// Open the gloss card for a mark (carries the `GlossMark` as detail).
#[cfg(all(format_runtime, target_arch = "wasm32"))]
pub const GLOSS_OPEN_EVENT: &str = "mareader:gloss-open";
/// Ask for a mark's remove menu (carries the `ContextTarget` as detail).
#[cfg(all(format_runtime, target_arch = "wasm32"))]
pub const GLOSS_CONTEXT_EVENT: &str = "mareader:gloss-context";
/// Internal link jump, dispatched by the engine's link layer.
#[cfg(all(format_runtime, target_arch = "wasm32"))]
pub const NAVIGATE_EVENT: &str = "mareader:navigate";
/// Page-range selection from the engine's thumbnail/id scanner.
#[cfg(all(format_runtime, target_arch = "wasm32"))]
pub const SELECTION_PAGES_EVENT: &str = "mareader:selection-pages";
/// Text-selection detail, dispatched by the engine's text layer.
#[cfg(all(format_runtime, target_arch = "wasm32"))]
pub const SELECTION_DETAIL_EVENT: &str = "mareader:selection-detail";
/// One-shot "scroll the sidebar to where the reader is" gesture.
pub const REVEAL_ACTIVE_EVENT: &str = "mareader:reveal-active";
/// Ask the library's title-bar search to take focus. Dispatched by the global
/// Cmd/Ctrl+F when no document is open — the shortcut means "search what you are
/// looking at", and on the library page that is the shelf, not a document. A
/// window event rather than a signal because the bar owns its own input node and
/// the shortcut layer must not know the library page exists.
pub const FOCUS_LIBRARY_SEARCH_EVENT: &str = "mareader:focus-library-search";

/// Dispatch a typed CustomEvent on `window` with `payload` as its detail.
pub fn dispatch_typed_event<T: Serialize>(name: &str, payload: &T) {
    let Some(win) = web_sys::window() else {
        return;
    };
    let Ok(detail) = serde_wasm_bindgen::to_value(payload) else {
        return;
    };
    let init = web_sys::CustomEventInit::new();
    init.set_detail(&detail);
    if let Ok(ev) = web_sys::CustomEvent::new_with_event_init_dict(name, &init) {
        let _ = win.dispatch_event(&ev);
    }
}

/// Dispatch a payload-less CustomEvent on `window` — a one-shot gesture with
/// no state to carry (e.g. [`REVEAL_ACTIVE_EVENT`]).
pub fn dispatch_event(name: &str) {
    let Some(win) = web_sys::window() else {
        return;
    };
    if let Ok(ev) = web_sys::CustomEvent::new(name) {
        let _ = win.dispatch_event(&ev);
    }
}
