//! Text-selection page-range tracking.
//!
//! The engine's selectionchange listener walks the DOM from the selection's
//! anchor and focus up to the nearest page host, parses the page index from
//! its id, and dispatches a `mareader:selection-pages` CustomEvent with
//! `{ first, last }` (1-based, inclusive) — or `null` to clear.
//!
//! This effect is the single place that turns the event into a write on
//! `state.viewer.selected_pages`, which
//! `features::reader::virtualizers` merges into the virtualizer's PINNED
//! window so those pages stay mounted.

use leptos::prelude::*;
use wasm_bindgen::JsValue;

use ui_kit::hooks::use_custom_event::use_raw_event;
use crate::state::ReaderState;

/// The JS protocol of the `mareader:selection-pages` event detail: `null`
/// (clear) or `{ first, last }` — 1-based, inclusive. One typed decoder for
/// the whole protocol, so the effect below stays about reactivity, not about
/// picking fields off a `JsValue`.
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

pub fn page_selection(state: ReaderState) {
    use_raw_event(ui_kit::events::SELECTION_PAGES_EVENT, move |detail| {
        match parse_selection(detail) {
            Some((first, last)) => {
                let total = state.document.num_pages.get_untracked().max(1);
                let f = first.clamp(1, total);
                let l = last.clamp(1, total);
                state.viewer.selected_pages.set(Some((f.min(l), f.max(l))));
            }
            None => state.viewer.selected_pages.set(None),
        }
    });
}
