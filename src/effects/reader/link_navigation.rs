//! Internal PDF link navigation.
//!
//! The engine's link layer cannot navigate by itself: page position is Rust
//! state (`viewer.page`), and the scroll/settle machinery in `navigation_sync`
//! is what makes a jump land cleanly instead of fighting the scroll observer.
//! An internal link dispatches a `mareader:navigate` CustomEvent and this
//! effect is the single place that turns it into a page change — the same
//! entry point the outline and thumbnails use.
//!
//! External links are NOT handled here: they are real `<a href
//! target=_blank>` elements, opened by the browser (or Tauri's shell) with no
//! Rust involvement.

use leptos::prelude::*;

use crate::components::primitives::hooks::use_custom_event::use_raw_event;
use crate::state::AppState;

pub fn link_navigation(state: AppState) {
    use_raw_event(crate::events::NAVIGATE_EVENT, move |detail| {
        let Some(page) = js_sys::Reflect::get(detail, &"page".into())
            .ok()
            .and_then(|v| v.as_f64())
        else {
            return;
        };
        let total = state.reader.document.num_pages.get_untracked().max(1);
        let page = (page as u32).clamp(1, total);
        state.reader.viewer.page.set(page);
    });
}
