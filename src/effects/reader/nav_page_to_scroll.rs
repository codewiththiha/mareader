//! Page → scroll: an explicit page jump moves `scroll_top` and `#page-list`
//! to the page top. Heights that aren't measured yet fall back to the same
//! uniform estimate PageList uses.

use leptos::prelude::*;

use pdf_core::layout::{DocumentLayout, ViewMode};
use crate::state::ReaderState;
use crate::components::document::dom_helpers::page_list;

use super::navigation_sync::{estimated_top, list_or_container_h, scroll_to, NavSyncState, JUMP_SETTLE_MS};

pub(super) fn page_to_scroll(
    state: ReaderState,
    layout: Memo<DocumentLayout>,
    nav: &NavSyncState,
) {
    let mode = state.viewer.mode;
    let page = state.viewer.page;
    let scroll_top = state.viewer.scroll_top;
    let heights = state.document.metrics.css_heights;
    let suppress = nav.suppress.clone();
    let pending = nav.pending.clone();
    let last_ours = nav.last_ours.clone();
    let jump_settle = nav.jump_settle.clone();
    Effect::new(move || {
        let p = page.get();
        let continuous = mode.get() == ViewMode::Continuous;
        if !continuous {
            return;
        }
        if suppress.get() {
            suppress.set(false);
            return;
        }
        let empty = heights.with_untracked(|hs| hs.is_empty());
        let target_top = if empty {
            estimated_top(p, state)
        } else {
            layout.with(|l| l.page_top(p.saturating_sub(1) as usize))
        };
        let target_px = target_top.ceil();
        let vh_now = list_or_container_h(state);
        if empty || layout.with(|l| l.dominant(scroll_top.get_untracked(), vh_now)) != p {
            scroll_top.set(target_px);
        }
        if let Some(list) = page_list()
            && (empty || layout.with(|l| l.dominant(list.scroll_top() as f64, vh_now)) != p)
        {
            let cur = list.scroll_top() as f64;
            let vh = list.client_height() as f64;
            let smooth = vh > 0.0 && (target_px - cur).abs() <= 2.0 * vh;
            scroll_to(&list, target_px, smooth);
            if smooth {
                jump_settle.set(js_sys::Date::now() + JUMP_SETTLE_MS);
            }
        }
        pending.set(Some(p));
        last_ours.set(target_px);
    });
}
