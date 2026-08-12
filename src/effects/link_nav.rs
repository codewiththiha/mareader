//! Internal PDF link navigation.
//!
//! The engine's link layer cannot navigate by itself: page position is Rust
//! state (`viewer.page`), and the scroll/settle machinery in `page_tracking`
//! is what makes a jump land cleanly instead of fighting the scroll observer.
//! So an internal link dispatches a `pdfreader:navigate` CustomEvent and this
//! effect is the single place that turns it into a page change — the same
//! entry point the outline and thumbnails already use.
//!
//! External links are NOT handled here. They are real `<a href target=_blank>`
//! elements, so the browser (or Tauri's shell) opens them with no Rust
//! involvement.

use leptos::prelude::*;
use wasm_bindgen::prelude::Closure;
use wasm_bindgen::JsCast;

use crate::core::state::AppState;

pub fn link_nav(state: AppState) {
    let Some(win) = web_sys::window() else { return };

    let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
        let Some(ce) = ev.dyn_ref::<web_sys::CustomEvent>() else {
            return;
        };
        let detail = ce.detail();
        let Some(page) = js_sys::Reflect::get(&detail, &"page".into())
            .ok()
            .and_then(|v| v.as_f64())
        else {
            return;
        };
        // A malformed or stale destination must not scroll the reader into the
        // void; clamp to the document rather than trusting the event.
        let total = state.doc.num_pages.get_untracked().max(1);
        let page = (page as u32).clamp(1, total);
        state.viewer.page.set(page);
    });

    let _ = win.add_event_listener_with_callback(
        "pdfreader:navigate",
        handler.as_ref().unchecked_ref(),
    );
    // The listener must outlive this call. The app root installs it once and
    // it lives for the whole session, so leaking the closure deliberately is
    // the correct lifetime here (the alternative, storing it in a signal, adds
    // ceremony for something that is never removed).
    handler.forget();
}
