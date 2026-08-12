//! Search pipeline: build index once, run search on submit, jump to results,
//! force visible-page re-render to re-apply highlights. OWNED BY branch C
//! (panels/sidebar).

use leptos::prelude::*;

use crate::api::engine;
use crate::core::layout::{page_top_css, ViewMode, PAGE_GAP};
use crate::core::state::AppState;
use crate::util::dom::page_list;

/// Build the search index (once), run the query, store the results, then nudge
/// `render_scale` so mounted PageCanvases re-render and the engine re-applies
/// its stored highlights (the engine keeps `highlightsByPage` between queries).
pub async fn run_search(state: AppState) {
    if !state.search.index_built.get() {
        match engine::build_search_index().await {
            Ok(_) => state.search.index_built.set(true),
            Err(e) => {
                web_sys::console::log_1(&format!("[search] build index: {e}").into());
                return;
            }
        }
    }

    let query = state.search.query.get();
    if query.trim().is_empty() {
        return;
    }

    match engine::search(&query).await {
        Ok(resp) => {
            state.search.total.set(resp.total);
            // `active` must stay within `results` bounds (contract for the
            // floating-search counter / prev-next): only mark the first result
            // active when there actually are results.
            let results = resp.results;
            state.search.active.set((!results.is_empty()).then_some(0));
            state.search.results.set(results);
            // Force mounted PageCanvases to re-render so the engine re-applies
            // its stored highlights to the freshly searched query.
            state.viewer.render_scale.update(|s| *s += 0.0001);
        }
        Err(e) => {
            web_sys::console::log_1(&format!("[search] query: {e}").into());
        }
    }
}

/// Jump to a search result. Single mode: set the current page (its PageCanvas
/// re-renders, applying highlights). Continuous mode: scroll `#page-list` to the
/// page top and nudge `render_scale` so mounted pages re-render.
pub fn jump_to_result(state: AppState, page: u32) {
    if state.viewer.mode.get() == ViewMode::Single {
        state.viewer.page.set(page);
    } else {
        let heights = state.doc.page_heights.get();
        let top = page_top_css(page.saturating_sub(1) as usize, &heights, PAGE_GAP);
        if let Some(list) = page_list()
        {
            list.set_scroll_top(top as i32);
        }
        state.viewer.render_scale.update(|s| *s += 0.0001);
    }
}

/// Advance the active search result by `dir` (±1) and jump to its page.
#[allow(dead_code)] // consumed by organisms::floating_search in phase 2
pub fn search_navigate(state: AppState, dir: i32) {
    let results = state.search.results.get();
    let Some(next) = crate::core::search::next_search_index(results.len(), state.search.active.get(), dir) else {
        return;
    };
    state.search.active.set(Some(next));
    if let Some(r) = results.get(next) {
        jump_to_result(state, r.page);
    }
}
