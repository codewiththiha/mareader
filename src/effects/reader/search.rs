//! Search pipeline: build the index once, run the query as the reader types,
//! and step through individual matches — scrolling each one into view rather
//! than jumping to the top of its page.
//!
//! The pipeline forks by format at [`run_search`] and nowhere else: PDF
//! indexes through the engine, text documents scan their own blocks in
//! Rust. Both tails converge on the same flat `SearchMatch` list, so the
//! results UI and the match-stepping maths below serve either.
//!
//! The two tails answer "where is this hit" differently, and each `SearchMatch`
//! carries the half its format has: a PDF a rect in page space, which the engine
//! multiplies by the scale and paints into the text layer; a reflowable document
//! a block and an occurrence ordinal (`block_hit`), which the row that renders
//! the block turns into boxes over its own rendered text
//! ([`crate::components::formats::reflow::highlight`]). A text match therefore
//! carries no rect, which the reveal path tolerates — a zero rect reveals the
//! page's top.

use std::collections::HashMap;

use leptos::prelude::*;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};

use app_chrome::hooks::dom::{h_page_list, page_list};
use app_chrome::TITLE_BAR_H;
use crate::state::ReaderState;
use reader_core::view::ViewMode;
use reader_core::search::{BlockHit, SearchMatch, scroll_to_reveal};
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
    if state.reflowable_untracked() {
        run_reflow_search(state);
        return;
    }
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

/// The reflowable tail of the pipeline: scan the open document's blocks, map each
/// hit through the current page cut, and publish the same flat match list
/// the engine tail produces. No index to build — the document IS the index
/// — and no engine round-trip at all.
fn run_reflow_search(state: ReaderState) {
    let query = state.search.query.get_untracked();
    if query.trim().is_empty() {
        clear_search(state);
        return;
    }
    let blocks = state.document.content.reflow.blocks.get_untracked();
    let hits = reflow_core::search::find_matches(&blocks, &query);
    let block_page = state.document.content.reflow.block_page.get_untracked();
    // The per-page occurrence ordinal the PDF side gets from the engine;
    // here it is bookkeeping the results list keeps for parity.
    let mut ordinal: HashMap<u32, u32> = HashMap::new();
    let mut matches = Vec::with_capacity(hits.len());
    for hit in hits {
        let page = block_page.get(hit.block).map_or(1, |p| p + 1);
        let index = ordinal.entry(page).and_modify(|n| *n += 1).or_insert(0);
        matches.push(SearchMatch {
            page,
            index: *index,
            text: hit.snippet.into(),
            // No rect: a reflowable page is re-cut by every typography knob, so
            // the durable answer is the block and the occurrence inside it, and
            // the row that renders the block finds the pixels.
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
    // The text pipeline has no build step; keep the flag honest so a
    // document switch reads it correctly either way.
    state.search.index_built.set(true);
}

pub fn clear_search(state: ReaderState) {
    // A reflowable document's boxes are painted by the rows themselves, off the
    // query and the match list below, so there is nothing to clear on the engine
    // side — and the call must not reach an engine that has no document.
    if !state.reflowable_untracked() {
        engine::clear_highlights();
    }
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
    // Only the engine has to be TOLD which match is current: it owns the boxes
    // it paints into the page's text layer. A reflowable document's rows read
    // `search.active` themselves and re-class the box that answers to it.
    if !state.reflowable_untracked() {
        engine::set_active_match(m.page, m.index as i32);
    }

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
        let before: f64 = state.document.content.metrics.intrinsic.with_untracked(|sizes| {
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

    // The text tail of the vertical branch: the stream scrolls BLOCKS, so
    // the match reveals through the stream's own virtualizer. (The page-cut
    // virtualizer this function was handed has no container in this mode;
    // its offsets describe a layout nothing is rendering.)
    //
    // A text hit carries no rect, so this is as precise as a reflowable
    // document gets: the block the match is in, at the top of the viewport.
    // That block is the one the match itself names; the page's first block is
    // the fallback for a match whose cut has since been repacked by a
    // typography change, where the stored block may no longer be the one on
    // screen.
    if state.reflowable_untracked() {
        let Some(stream) = state.document.content.reflow.stream_handle() else {
            return;
        };
        let block = m.block_hit.map_or_else(
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

    let Some(list) = page_list() else {
        return;
    };
    let scale = state.viewer.zoom.visual_scale();
    let page_top = virtualizer.offset_of(m.page.saturating_sub(1) as usize);

    // The strip starts at the scroller's origin (no toolbar band above the
    // first page), so a match's scroll position is the page's own offset plus
    // its position on the page. The overlay bar and search bar still cover
    // the top of the VIEWPORT, which is what the reveal inset below models.
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
