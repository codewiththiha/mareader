//! Search pipeline: build the index once, run the query as the reader types,
//! and step through individual matches — scrolling each one into view rather
//! than jumping to the top of its page. OWNED BY branch C (panels/sidebar).

use leptos::prelude::*;

use pdf_engine::api as engine;
use pdf_core::layout::{page_top_css, ViewMode, PAGE_GAP, TOOLBAR_H};
use pdf_core::search::{scroll_to_reveal, SearchMatch};
use crate::state::ViewerState;
use crate::dom::page_list;

/// Height of the floating search bar plus its gap, in CSS px. The bar hangs
/// over the top-right of the viewer, so a match revealed underneath it would be
/// covered; the reveal maths treats this as dead space.
///
/// Keep in sync with `FloatingSearch`'s `top-14` (56px) plus its ~48px body.
const SEARCH_BAR_H: f64 = 104.0;

/// Breathing room left around a revealed match.
const REVEAL_MARGIN: f64 = 24.0;

/// Run the query and store the flat match list.
///
/// Does NOT navigate and does NOT choose an active match — callers decide
/// whether typing should move the view (it shouldn't) or Enter should (it
/// should). The engine keeps the query so newly mounted pages highlight
/// themselves as they render.
pub async fn run_search(state: ViewerState) {
    if !state.search.index_built.get_untracked() {
        match engine::build_search_index().await {
            Ok(_) => state.search.index_built.set(true),
            Err(e) => {
                web_sys::console::log_1(&format!("[search] build index: {e}").into());
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
            // The previous query's cursor is meaningless now. Highlights for
            // the new query are painted by the engine on the pages already
            // mounted, so nothing needs a re-render.
            state.search.active.set(None);
            engine::set_active_match(0, -1);
        }
        Err(e) => {
            web_sys::console::log_1(&format!("[search] query: {e}").into());
        }
    }
}

/// Drop the query, its matches and every painted highlight.
pub fn clear_search(state: ViewerState) {
    engine::clear_highlights();
    state.search.total.set(0);
    state.search.matches.set(Vec::new());
    state.search.active.set(None);
    state.search.dismissed.set(false);
}

/// Close the floating search bar. Highlights remain visible in the document.
///
/// Search highlights persist during reading and scrolling until the user
/// explicitly clears or changes the search query. Dismissing the bar just
/// hides the UI — the highlights, query, and matches all stay so the reader
/// can reopen the bar (`resume_search`) and pick up where they left off.
pub fn dismiss_search(state: ViewerState) {
    state.search.visible.set(false);
}

/// Reopen the search bar. The query and highlights are still intact from the
/// last search (they were never cleared on dismiss).
pub fn resume_search(state: ViewerState) {
    state.search.visible.set(true);
}

/// Scroll `m` into view and mark it as the current match.
///
/// Continuous mode scrolls the column so the match itself is inside the
/// readable band (see `scroll_to_reveal`); if it is already comfortably
/// visible, nothing moves. Single-page mode just turns to its page.
pub fn reveal_match(state: ViewerState, m: &SearchMatch) {
    // Tag first: the emphasis should land even if the view does not move.
    engine::set_active_match(m.page, m.index as i32);

    if state.viewer.mode.get_untracked() == ViewMode::Single {
        state.viewer.page.set(m.page);
        return;
    }

    let Some(list) = page_list() else { return };
    let heights = state.doc.page_heights.get_untracked();
    let scale = state.viewer.render_scale.get_untracked();
    let page_top = page_top_css(m.page.saturating_sub(1) as usize, &heights, PAGE_GAP);

    // Match rects are scale-1; the column is laid out at the render scale.
    //
    // `page_top_css` is measured inside the column wrapper, which is offset by
    // TOOLBAR_H (the `mt-12` that lets pages travel under the glass header).
    // Adding it converts to the scroll container's own coordinates, which is
    // what `scroll_top` and the insets below are expressed in. Leaving it out
    // put every match 48 px higher than it really is, so a hit just past the
    // fold looked visible and the view stayed put.
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
        list.set_scroll_top(next as i32);
    }
}

/// Select match `index` (bounds-checked) and reveal it.
pub fn activate_match(state: ViewerState, index: usize) {
    let matches = state.search.matches.get_untracked();
    let Some(m) = matches.get(index) else { return };
    state.search.active.set(Some(index));
    reveal_match(state, m);
}

/// Step to the next/previous MATCH (not page) and reveal it, wrapping at the
/// ends of the document.
pub fn search_navigate(state: ViewerState, dir: i32) {
    let len = state.search.matches.with_untracked(Vec::len);
    let Some(next) =
        pdf_core::search::next_search_index(len, state.search.active.get_untracked(), dir)
    else {
        return;
    };
    activate_match(state, next);
}

/// Whether a keypress means "the reader moved on", ending the grace period.
///
/// Kept for the test below even though `dismissed_search_watch` is now a no-op
/// (highlights persist until explicitly cleared, not auto-wiped on keypress).
#[allow(dead_code)]
fn key_ends_grace(key: &str, cmd_or_ctrl: bool) -> bool {
    let bare_modifier = matches!(key, "Control" | "Shift" | "Alt" | "Meta");
    let reopening = cmd_or_ctrl && key.eq_ignore_ascii_case("f");
    key != "Escape" && !bare_modifier && !reopening
}

/// Watcher for search keyboard shortcuts / dismissal.
///
/// Left empty: search results and highlights now persist cleanly during
/// continuous scrolling until explicitly cleared by the reader (changing the
/// query, clearing the search, or opening a new document). The previous
/// version attached scroll/pointerdown/keydown listeners that auto-wiped
/// highlights the moment the reader scrolled past a dismissed search —
/// that was the "search highlighters disappear forever on scroll" bug.
pub fn dismissed_search_watch(_state: ViewerState) {
    // No-op. Highlights persist until the reader explicitly clears or changes
    // the search query (see `clear_search` and `run_search`).
}

#[cfg(test)]
mod tests {
    use super::key_ends_grace;

    /// Reading on ends the grace period; reaching back for the search does not.
    #[test]
    fn only_moving_on_ends_the_grace_period() {
        // (key, cmd_or_ctrl, ends_grace)
        let cases: &[(&str, bool, bool)] = &[
            // Reading / editing the document = moved on.
            ("ArrowDown", false, true),
            ("PageDown", false, true),
            (" ", false, true),
            ("a", false, true),
            ("f", false, true), // plain f is typing, not the shortcut
            // Coming back to the search, or not an interaction at all.
            ("Escape", false, false),
            ("f", true, false),
            ("F", true, false), // Shift+Cmd+F still reopens
            ("Control", false, false),
            ("Meta", true, false),
            ("Shift", false, false),
            ("Alt", false, false),
        ];
        for &(key, mods, want) in cases {
            assert_eq!(key_ends_grace(key, mods), want, "key={key:?} cmd_or_ctrl={mods}");
        }
    }
}
