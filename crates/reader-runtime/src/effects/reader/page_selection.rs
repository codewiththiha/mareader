//! Text-selection page-range tracking.

use leptos::prelude::*;
use wasm_bindgen::JsValue;

use app_ui::components::primitives::hooks::use_custom_event::use_raw_event_from;

use crate::pane::origin::{Origin, origin_of};

/// The event detail's protocol: `null` or `{ first, last }`.
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

/// A range bubbles from the page host; a clear rides the window.
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
                // Notify only on a real change.
                None => {
                    if state.reader.viewer.selected_pages.get_untracked().is_some() {
                        state.reader.viewer.selected_pages.set(None);
                    }
                }
            }
        },
    );
}
