//! The app's window-`CustomEvent` protocol: every event name in one table,
//! plus the one typed dispatcher.
//!
//! Window CustomEvents are the app's one cross-cutting message mechanism —
//! they cross layer boundaries (services → components, engine JS → Rust)
//! without either side holding a signal from the other. Some of these names
//! are also a protocol with the imperative engine under `public/engine/`,
//! which dispatches `pdfreader:navigate`, `pdfreader:selection-pages` and
//! `pdfreader:selection-detail` from plain JS; renaming one means renaming it
//! there too.

use serde::Serialize;

/// AI chunk stream, bridged from the Tauri backend by `services::ai`.
pub const AI_CHUNK_EVENT: &str = "pdfreader:ai-chunk";
/// Open the gloss card for a mark (carries the `GlossMark` as detail).
pub const GLOSS_OPEN_EVENT: &str = "pdfreader:gloss-open";
/// Ask for a mark's remove menu (carries the `ContextTarget` as detail).
pub const GLOSS_CONTEXT_EVENT: &str = "pdfreader:gloss-context";
/// Internal link jump, dispatched by the engine's link layer.
pub const NAVIGATE_EVENT: &str = "pdfreader:navigate";
/// Page-range selection from the engine's thumbnail/id scanner.
pub const SELECTION_PAGES_EVENT: &str = "pdfreader:selection-pages";
/// Text-selection detail, dispatched by the engine's text layer.
pub const SELECTION_DETAIL_EVENT: &str = "pdfreader:selection-detail";
/// One-shot "scroll the sidebar to where the reader is" gesture.
pub const REVEAL_ACTIVE_EVENT: &str = "pdfreader:reveal-active";

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
