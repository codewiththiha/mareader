//! Search pipeline: index on the first query, run as the reader types,
//! step through matches.

#[cfg(feature = "reflow")]
use std::collections::HashMap;

use leptos::prelude::*;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};

use crate::state::ReaderState;
use app_chrome::TITLE_BAR_H;
#[cfg(feature = "reflow")]
use reader_core::search::BlockHit;
use reader_core::search::{SearchMatch, scroll_to_reveal};
use reader_core::view::ViewMode;

/// The floating search bar's height plus its gap: dead space to the
/// reveal.
const SEARCH_BAR_H: f64 = 104.0;

/// Breathing room left around a revealed match.
const REVEAL_MARGIN: f64 = 24.0;

/// Run the query and store the flat match list.
pub async fn run_search(state: ReaderState) {
    // A build can outlive the reader that asked: the pane's generation is
    // the stand-down.
    if state.reflowable_now() {
        #[cfg(feature = "reflow")]
        run_reflow_search(state);
        return;
    }
    #[cfg(feature = "pdf")]
    run_pdf_search(state).await;
}

#[cfg(feature = "pdf")]
async fn run_pdf_search(state: ReaderState) {
    let pane = state.pane;
    let stamp = pane.generation();
    if !state.search.index_built.get_untracked() {
        // One build at a time: a second extraction is pure wasm churn.
        if state.search.building.get_untracked() {
            return;
        }
        state.search.building.set(true);
        // The pane's OWN session builds into its own index.
        let pdf = pane.pdf();
        let built = pdf
            .build_search_index(state.document.num_pages.get_untracked())
            .await;
        // Every session replacement claims a new generation, so this check
        // covers "same session".
        if !pane.owns_generation(stamp) {
            return;
        }
        state.search.building.set(false);
        match built {
            Ok(_) => {
                state.search.index_built.set(true);
                // The heap probe at the one step that scales with the book.
                app_state::memory::log_heap("search index");
            }
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

    if !pane.owns_generation(stamp) {
        return;
    }
    let pdf = pane.pdf();
    match pdf.search(&query) {
        Some(resp) => {
            state.search.total.set(resp.total);
            state.search.matches.set(resp.matches);
            state.search.active.set(None);
            pdf.set_active_match(0, -1);
        }
        None => {
            web_sys::console::warn_1(&"[search] query: the pane holds no PDF session".into());
        }
    }
}

/// The reflowable tail: scan the blocks, map hits through the cut,
/// publish the same list.
#[cfg(feature = "reflow")]
fn run_reflow_search(state: ReaderState) {
    let query = state.search.query.get_untracked();
    if query.trim().is_empty() {
        clear_search(state);
        return;
    }
    let blocks = state.document.content.reflow.blocks.get_untracked();
    let hits = reflow_core::search::find_matches(&blocks, &query);
    let block_page = state.document.content.reflow.block_page.get_untracked();
    // The per-page occurrence ordinal
    // the engine gives a PDF; here it is
    // bookkeeping.
    let mut ordinal: HashMap<u32, u32> = HashMap::new();
    let mut matches = Vec::with_capacity(hits.len());
    for hit in hits {
        let page = block_page.get(hit.block).map_or(1, |p| p + 1);
        let index = ordinal.entry(page).and_modify(|n| *n += 1).or_insert(0);
        matches.push(SearchMatch {
            page,
            index: *index,
            text: hit.snippet.into(),
            // No rect: block and occurrence are
            // durable; the row finds the pixels.
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            block_hit: Some(BlockHit {
                block: hit.block as u32,
                occurrence: hit.occurrence as u32,
            }),
        });
    }
    let total = matches.len() as u32;
    state.search.total.set(total);
    state.search.matches.set(matches);
    state.search.active.set(None);
    // No build step here; keep the flag honest for a document switch.
    state.search.index_built.set(true);
}

pub fn clear_search(state: ReaderState) {
    // Reflowable boxes are painted by the rows themselves; nothing to
    // clear.
    #[cfg(feature = "pdf")]
    if !state.reflowable_now() {
        state.pane.pdf().clear_highlights();
    }
    state.search.total.set(0);
    state.search.matches.set(Vec::new());
    state.search.active.set(None);
    state.search.dismissed.set(false);
}

pub fn dismiss_search(state: ReaderState) {
    // Hiding the bar disposes the overlay's owner, so this clear lets the
    // next search rebuild.
    state.search.building.set(false);
    state.search.visible.set(false);
}

pub fn resume_search(state: ReaderState) {
    state.search.visible.set(true);
}

fn reveal_match(state: ReaderState, virtualizer: &Virtualizer, m: &SearchMatch) {
    // Only the engine owns boxes to be told; rows read `search.active`
    // themselves.
    #[cfg(feature = "pdf")]
    if !state.reflowable_now() {
        state.pane.pdf().set_active_match(m.page, m.index as i32);
    }

    let mode = state.viewer.mode.get_untracked();
    // Dual is paginated like Single: setting the page shows the spread that
    // contains the match.
    if mode == ViewMode::Single || mode == ViewMode::Spread {
        state.viewer.page.set(m.page);
        return;
    }

    if mode == ViewMode::ScrollHorizontal {
        let Some(list) = state.dom.h_page_list() else {
            return;
        };
        let scale = state.viewer.zoom.visual_scale();
        let before: f64 = state
            .document
            .content
            .metrics
            .intrinsic
            .with_untracked(|sizes| {
                sizes
                    .iter()
                    .take((m.page - 1) as usize)
                    .map(|s| s.width)
                    .sum::<f64>()
            });
        let left = TITLE_BAR_H + before * scale + m.x * scale;
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

    // The text tail: the stream scrolls BLOCKS, so the match reveals through
    // its virtualizer.
    if state.reflowable_now() {
        let Some(stream) = state.document.content.reflow.stream_handle() else {
            return;
        };
        let block =
            m.block_hit.map_or_else(
                || {
                    state.document.content.reflow.cuts.with_untracked(|cuts| {
                        reflow_core::pager::first_block_of_page(cuts, m.page)
                    })
                },
                |hit| hit.block as usize,
            );
        stream.scroll_to_index(block, Align::Start, ScrollMode::Auto);
        return;
    }

    let Some(list) = state.dom.page_list() else {
        return;
    };
    let scale = state.viewer.zoom.visual_scale();
    let page_top = virtualizer.offset_of(m.page.saturating_sub(1) as usize);

    // A match's scroll position is its page's offset plus its place on the
    // page.
    let top = page_top + m.y * scale;
    let bottom = top + (m.h * scale).max(1.0);

    if let Some(next) = scroll_to_reveal(
        top,
        bottom,
        list.scroll_top() as f64,
        list.client_height() as f64,
        TITLE_BAR_H + SEARCH_BAR_H,
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
        reader_core::search::next_search_index(len, state.search.active.get_untracked(), dir)
    else {
        return;
    };
    activate_match(state, virtualizer, next);
}
