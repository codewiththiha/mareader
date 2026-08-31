//! Text-selection page-range tracking.
//!
//! The engine's selectionchange listener walks the DOM from the selection's
//! anchor and focus up to the nearest `.pdf-page` host, parses the page index
//! from its id (`cont-{i}-pg`), and dispatches a `pdfreader:selection-pages`
//! CustomEvent with `{ first, last }` (1-based, inclusive) — or `null` to
//! clear.
//!
//! This effect is the single place that turns that event into a write on
//! `state.reader.viewer.selected_pages`, which `PageList` reads to PIN those pages in
//! the virtualization window.

use leptos::prelude::*;
use wasm_bindgen::JsValue;

use crate::state::AppState;

/// The JS protocol of the `pdfreader:selection-pages` event detail:
/// `null` (clear) or `{ first, last }` — 1-based, inclusive.
///
/// One typed decoder for the whole protocol, so the effect below stays
/// about reactivity, not about picking fields off a `JsValue`.
fn parse_selection(detail: &JsValue) -> Option<(u32, u32)> {
    if detail.is_null() || detail.is_undefined() {
        return None;
    }
    let num = |key: &str| {
        js_sys::Reflect::get(detail, &key.into())
            .ok()
            .and_then(|v| v.as_f64())
            .map(|n| n as u32)
    };
    match (num("first"), num("last")) {
        (Some(f), Some(l)) => Some((f, l)),
        _ => None,
    }
}

pub fn page_selection(state: AppState) {
    let _handle = window_event_listener(
        leptos::ev::Custom::new(crate::events::SELECTION_PAGES_EVENT),
        move |ev: web_sys::CustomEvent| {
            let detail = ev.detail();
            match parse_selection(&detail) {
                Some((first, last)) => {
                    let total = state.reader.document.num_pages.get_untracked().max(1);
                    let f = first.clamp(1, total);
                    let l = last.clamp(1, total);
                    state.reader.viewer.selected_pages.set(Some((f.min(l), f.max(l))));
                }
                None => state.reader.viewer.selected_pages.set(None),
            }
        },
    );
    on_cleanup(move || _handle.remove());
}
