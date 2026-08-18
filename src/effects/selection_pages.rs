//! Text-selection page-range tracking.
//!
//! The `pdfEngine.ts` selectionchange listener walks the DOM from the
//! selection's anchor and focus up to the nearest `.pdf-page` host, parses the
//! page index from its id (`cont-{i}-pg`), and dispatches a
//! `pdfreader:selection-pages` CustomEvent with `{ first, last }` (1-based,
//! inclusive) — or `null` to clear.
//!
//! This effect is the single place that turns that event into a write on
//! `state.viewer.selected_pages`, which `PageList` reads to PIN those pages in
//! the virtualization window. Without pinning, scrolling evicts the page the
//! selection started on, orphaning its DOM nodes and breaking copy of
//! multi-page selections.

use leptos::prelude::*;
use wasm_bindgen::prelude::Closure;
use wasm_bindgen::JsCast;

use crate::core::state::AppState;

pub fn selection_pages(state: AppState) {
    let Some(win) = web_sys::window() else {
        return;
    };

    let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
        let Some(ce) = ev.dyn_ref::<web_sys::CustomEvent>() else {
            return;
        };
        let detail = ce.detail();
        // `detail` is `null` when the selection is cleared or collapsed.
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
                // Clamp to the document so a malformed/stale event can't
                // pin a non-existent page and wedge the virtualization window.
                let total = state.doc.num_pages.get_untracked().max(1);
                let f = (f as u32).clamp(1, total);
                let l = (l as u32).clamp(1, total);
                state.viewer.selected_pages.set(Some((f.min(l), f.max(l))));
            }
            _ => state.viewer.selected_pages.set(None),
        }
    });

    let _ = win.add_event_listener_with_callback(
        "pdfreader:selection-pages",
        handler.as_ref().unchecked_ref(),
    );
    // The listener must outlive this call. The app root installs it once and
    // it lives for the whole session, so leaking the closure deliberately is
    // the correct lifetime here (the alternative, storing it in a signal, adds
    // ceremony for something that is never removed).
    handler.forget();
}
