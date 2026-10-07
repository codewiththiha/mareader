//! The app's window-`CustomEvent` protocol: one name table and typed
//! dispatchers, checked against the engine's `events.ts`.

use serde::Serialize;

/// AI chunk stream, bridged from the Tauri backend by `services::ai`.
pub const AI_CHUNK_EVENT: &str = "mareader:ai-chunk";
/// Open the gloss card for a mark (carries the `GlossMark` as detail).
pub const GLOSS_OPEN_EVENT: &str = "mareader:gloss-open";
/// Ask for a mark's remove menu (carries the `ContextTarget` as detail).
pub const GLOSS_CONTEXT_EVENT: &str = "mareader:gloss-context";
/// Internal link jump, dispatched by the engine's link layer.
pub const NAVIGATE_EVENT: &str = "mareader:navigate";
/// Page-range selection from the engine's thumbnail/id scanner.
pub const SELECTION_PAGES_EVENT: &str = "mareader:selection-pages";
/// Text-selection detail, dispatched by the engine's text layer.
pub const SELECTION_DETAIL_EVENT: &str = "mareader:selection-detail";
/// One-shot "scroll the sidebar to where the reader is" gesture.
pub const REVEAL_ACTIVE_EVENT: &str = "mareader:reveal-active";
/// Ask the library's title-bar search to take focus.
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

/// Dispatch a typed CustomEvent on `target`, bubbling to `window`.
pub fn dispatch_typed_event_on<T: Serialize>(
    target: &web_sys::EventTarget,
    name: &str,
    payload: &T,
) {
    let Ok(detail) = serde_wasm_bindgen::to_value(payload) else {
        return;
    };
    let init = web_sys::CustomEventInit::new();
    init.set_detail(&detail);
    init.set_bubbles(true);
    if let Ok(ev) = web_sys::CustomEvent::new_with_event_init_dict(name, &init) {
        let _ = target.dispatch_event(&ev);
    }
}

/// Dispatch a payload-less CustomEvent on `window`.
pub fn dispatch_event(name: &str) {
    let Some(win) = web_sys::window() else {
        return;
    };
    if let Ok(ev) = web_sys::CustomEvent::new(name) {
        let _ = win.dispatch_event(&ev);
    }
}
