//! Scroll → page: the dominant page (the one filling most of the viewport)
//! writes `viewer.page`. Does not use the top-edge page — zooming out would
//! walk the counter while the reader was holding still.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;

use pdf_core::layout::{DocumentLayout, ViewMode};
use crate::state::ReaderState;
use crate::components::document::dom_helpers::page_list;

pub(super) fn scroll_to_page(
    state: ReaderState,
    layout: Memo<DocumentLayout>,
    suppress: Rc<Cell<bool>>,
) {
    let mode = state.viewer.mode;
    let page = state.viewer.page;
    let scroll_top = state.viewer.scroll_top;
    let heights = state.document.metrics.css_heights;
    Effect::new(move || {
        if mode.get() != ViewMode::Continuous {
            return;
        }
        let st = scroll_top.get();
        if heights.with(|hs| hs.is_empty()) {
            return;
        }
        let (_, cont_h) = state.viewer.container_size.get();
        let vh = page_list()
            .map(|el| el.client_height() as f64)
            .filter(|h| *h > 1.0)
            .unwrap_or(cont_h);
        let p = layout.with(|l| l.dominant(st, vh));
        if page.get_untracked() != p {
            suppress.set(true);
            page.set(p);
        }
    });
}
