//! Search pipeline: build index once, run search on submit, jump to results,
//! force visible-page re-render to re-apply highlights. OWNED BY branch C
//! (panels/sidebar).

use leptos::prelude::*;

use crate::api::engine;
use crate::core::layout::{page_top_css, ViewMode, PAGE_GAP};
use crate::core::state::AppState;

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
            state.search.results.set(resp.results);
            state.search.active.set(None);
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
        if let Some(list) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("page-list"))
        {
            list.set_scroll_top(top as i32);
        }
        state.viewer.render_scale.update(|s| *s += 0.0001);
    }
}
