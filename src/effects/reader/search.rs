//! Search pipeline: build the index once, run the query as the reader types,
//! and step through individual matches — scrolling each one into view rather
//! than jumping to the top of its page.

use leptos::prelude::*;
use virtual_list_leptos::{ScrollMode, Virtualizer};

use crate::components::primitives::hooks::dom::{h_page_list, page_list};
use crate::state::ReaderState;
use pdf_core::layout::{TOOLBAR_H, ViewMode};
use pdf_core::search::{SearchMatch, scroll_to_reveal};
use pdf_engine::api as engine;

/// Height of the floating search bar plus its gap, in CSS px. The bar hangs
/// over the top-right of the viewer, so a match revealed underneath it would be
/// covered; the reveal maths treats this as dead space.
///
/// Keep in sync with `FloatingSearch`'s `top-14` (56px) plus its ~48px body.
const SEARCH_BAR_H: f64 = 104.0;

/// Breathing room left around a revealed match.
const REVEAL_MARGIN: f64 = 24.0;

/// Run the query and store the flat match list.
pub async fn run_search(state: ReaderState) {
    if !state.search.index_built.get_untracked() {
        // The index build extracts ~3 pages per turn (see
        // pdf_engine::api::search::SEARCH_PAGE_CONCURRENCY); the page count
        // comes from the open flow, which alone knows the document size.
        match engine::build_search_index(state.document.num_pages.get_untracked()).await {
            Ok(_) => state.search.index_built.set(true),
            Err(e) => {
                web_sys::console::warn_1(&format!("[search] build index: {e}").into());
                return;
            }
        }
    }

    let query = state.search.query.get_untracked();
    if query.trim().is_empty() {
        clear_search(state);
        return;
    }

    match engine::search(&query).await {
        Ok(resp) => {
            state.search.total.set(resp.total);
            state.search.matches.set(resp.matches);
            state.search.active.set(None);
            engine::set_active_match(0, -1);
        }
        Err(e) => {
            web_sys::console::warn_1(&format!("[search] query: {e}").into());
        }
    }
}

pub fn clear_search(state: ReaderState) {
    engine::clear_highlights();
    state.search.total.set(0);
    state.search.matches.set(Vec::new());
    state.search.active.set(None);
    state.search.dismissed.set(false);
}

pub fn dismiss_search(state: ReaderState) {
    state.search.visible.set(false);
}

pub fn resume_search(state: ReaderState) {
    state.search.visible.set(true);
}

pub fn reveal_match(state: ReaderState, virtualizer: &Virtualizer, m: &SearchMatch) {
    engine::set_active_match(m.page, m.index as i32);

    let mode = state.viewer.mode.get_untracked();
    // Dual is paginated like Single: setting the page shows the spread that
    // contains the match.
    if mode == ViewMode::Single || mode == ViewMode::Spread {
        state.viewer.page.set(m.page);
        return;
    }

    if mode == ViewMode::ScrollHorizontal {
        let Some(list) = h_page_list() else {
            return;
        };
        let scale = state.viewer.zoom.visual_scale();
        let before: f64 = state.document.metrics.intrinsic.with_untracked(|sizes| {
            sizes
                .iter()
                .take((m.page - 1) as usize)
                .map(|s| s.width)
                .sum::<f64>()
        });
        let left = TOOLBAR_H + before * scale + m.x * scale;
        let right = left + (m.w * scale).max(1.0);
        if let Some(next) = scroll_to_reveal(
            left,
            right,
            list.scroll_left() as f64,
            list.client_width() as f64,
            0.0,
            0.0,
            48.0,
        ) {
            list.set_scroll_left(next as i32);
        }
        return;
    }

    let Some(list) = page_list() else {
        return;
    };
    let scale = state.viewer.zoom.visual_scale();
    let page_top = virtualizer.offset_of(m.page.saturating_sub(1) as usize);

    let top = TOOLBAR_H + page_top + m.y * scale;
    let bottom = top + (m.h * scale).max(1.0);

    if let Some(next) = scroll_to_reveal(
        top,
        bottom,
        list.scroll_top() as f64,
        list.client_height() as f64,
        TOOLBAR_H + SEARCH_BAR_H,
        0.0,
        REVEAL_MARGIN,
    ) {
        virtualizer.scroll_to_offset(next, ScrollMode::Instant);
    }
}

pub fn activate_match(state: ReaderState, virtualizer: &Virtualizer, index: usize) {
    let Some(m) = state
        .search
        .matches
        .with_untracked(|matches| matches.get(index).cloned())
    else {
        return;
    };
    state.search.active.set(Some(index));
    reveal_match(state, virtualizer, &m);
}

pub fn search_navigate(state: ReaderState, virtualizer: &Virtualizer, dir: i32) {
    let len = state.search.matches.with_untracked(Vec::len);
    let Some(next) =
        pdf_core::search::next_search_index(len, state.search.active.get_untracked(), dir)
    else {
        return;
    };
    activate_match(state, virtualizer, next);
}
