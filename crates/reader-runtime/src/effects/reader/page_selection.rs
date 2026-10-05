//! Text-selection page-range tracking.
//!
//! The engine's selectionchange listener walks the DOM from the selection's
//! anchor and focus up to the nearest page host, parses the page index from
//! its id, and dispatches a `mareader:selection-pages` CustomEvent with
//! `{ first, last }` (1-based, inclusive) — or `null` to clear.
//!
//! This effect is the single place that turns the event into a write on
//! `state.reader.viewer.selected_pages`, which
//! `crate::features::virtualizers` merges into the virtualizer's PINNED
//! window so those pages stay mounted.

use leptos::prelude::*;
use wasm_bindgen::JsValue;

use app_ui::components::primitives::hooks::use_custom_event::use_raw_event_from;

use crate::pane::origin::{Origin, origin_of};

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

/// A range is dispatched on the selection's page host and bubbles; a clear
/// on the window. The document has ONE selection, so a range in another
/// pane means none in this one: that pane's pin is released here.
pub fn page_selection(state: crate::context::ReaderContext, active: Signal<bool>) {
    use_raw_event_from(
        app_ui::events::SELECTION_PAGES_EVENT,
        move |detail, origin| {
            let active = active.try_get_untracked().unwrap_or(false);
            let mine = origin_of(&state.reader.dom, active, origin.as_ref()) == Origin::Mine;
            match parse_selection(detail).filter(|_| mine) {
                Some((first, last)) => {
                    let total = state.reader.document.num_pages.get_untracked().max(1);
                    let f = first.clamp(1, total);
                    let l = last.clamp(1, total);
                    state
                        .reader
                        .viewer
                        .selected_pages
                        .set(Some((f.min(l), f.max(l))));
                }
                // Every other pane hears each selection as a clear: notify only
                // on a real change, so they do not re-pin for nothing.
                None => {
                    if state.reader.viewer.selected_pages.get_untracked().is_some() {
                        state.reader.viewer.selected_pages.set(None);
                    }
                }
            }
        },
    );
}
