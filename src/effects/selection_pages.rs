//! Text-selection page-range tracking.
//!
//! The engine's selectionchange listener walks the DOM from the selection's
//! anchor and focus up to the nearest `.pdf-page` host, parses the page index
//! from its id (`cont-{i}-pg`), and dispatches a `pdfreader:selection-pages`
//! CustomEvent with `{ first, last }` (1-based, inclusive) — or `null` to
//! clear.
//!
//! This effect is the single place that turns that event into a write on
//! `state.viewer.selected_pages`, which `PageList` reads to PIN those pages in
//! the virtualization window.

use leptos::prelude::*;

use crate::core::state::AppState;

pub fn selection_pages(state: AppState) {
    let _handle = window_event_listener(
        leptos::ev::Custom::new("pdfreader:selection-pages"),
        move |ev: web_sys::CustomEvent| {
            let detail = ev.detail();
            if detail.is_null() {
                state.viewer.selected_pages.set(None);
                return;
            }
            let first = js_sys::Reflect::get(&detail, &"first".into())
                .ok()
                .and_then(|v| v.as_f64());
            let last = js_sys::Reflect::get(&detail, &"last".into())
                .ok()
                .and_then(|v| v.as_f64());
            match (first, last) {
                (Some(f), Some(l)) => {
                    let total = state.doc.num_pages.get_untracked().max(1);
                    let f = (f as u32).clamp(1, total);
                    let l = (l as u32).clamp(1, total);
                    state.viewer.selected_pages.set(Some((f.min(l), f.max(l))));
                }
                _ => state.viewer.selected_pages.set(None),
            }
        },
    );
    on_cleanup(move || _handle.remove());
}
