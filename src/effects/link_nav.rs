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

use crate::core::state::AppState;

pub fn link_nav(state: AppState) {
    // `window_event_listener` attaches to the current reactive owner (the app
    // root) and removes itself on dispose — no `handler.forget()` leak.
    let _handle = window_event_listener(
        leptos::ev::Custom::new("pdfreader:navigate"),
        move |ev: web_sys::CustomEvent| {
            let detail = ev.detail();
            let Some(page) = js_sys::Reflect::get(&detail, &"page".into())
                .ok()
                .and_then(|v| v.as_f64())
            else {
                return;
            };
            let total = state.doc.num_pages.get_untracked().max(1);
            let page = (page as u32).clamp(1, total);
            state.viewer.page.set(page);
        },
    );
    on_cleanup(move || _handle.remove());
}
