//! Search pipeline: build the index once, run the query as the reader types,
//! and step through individual matches — scrolling each one into view rather
//! than jumping to the top of its page. OWNED BY branch C (panels/sidebar).

use std::cell::Cell;

use leptos::prelude::*;

type ScrollClosureSlot = leptos::prelude::StoredValue<
    Option<wasm_bindgen::closure::Closure<dyn Fn(web_sys::Event)>>,
    leptos::prelude::LocalStorage,
>;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

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

thread_local! {
    /// Set for the remainder of the current task whenever a search is
    /// dismissed: the dismissing gesture (Escape / pointerdown outside) is
    /// also seen by the watcher below, which would otherwise discard the
    /// trace in the very same event. Cleared on a microtask so the protection
    /// never outlives the dispatching task.
    static JUST_DISMISSED: Cell<bool> = const { Cell::new(false) };
}

/// Close the bar but LEAVE the highlights on screen, muted.
///
/// Dismissing a search is rarely the end of it — the reader wants the page
/// back, not the results gone, and wiping everything on Escape means retyping
/// the query to see the hits again. So the boxes stay in a stale colour and the
/// query is kept: reopening the bar (`resume_search`) restores it exactly.
/// The first real interaction with the document ends the grace period
/// (`discard_dismissed`).
///
/// Nothing to dismiss without a query, in which case this is a plain close.
pub fn dismiss_search(state: ViewerState) {
    state.search.visible.set(false);
    if state.search.matches.with_untracked(Vec::is_empty) {
        clear_search(state);
        state.search.query.set(String::new());
        return;
    }
    engine::set_highlight_mode(true);
    state.search.dismissed.set(true);
    JUST_DISMISSED.with(|g| g.set(true));
    queue_microtask(|| JUST_DISMISSED.with(|g| g.set(false)));
}

/// Reopen the bar. A dismissed-but-not-yet-discarded search comes back intact,
/// query and all; anything else opens fresh.
pub fn resume_search(state: ViewerState) {
    if state.search.dismissed.get_untracked() {
        engine::set_highlight_mode(false);
        state.search.dismissed.set(false);
    }
    state.search.visible.set(true);
}

/// End the grace period: the reader has moved on, so the stale highlights and
/// the query go away. A no-op unless a dismissed search is actually pending,
/// which is what makes it cheap enough to call from a scroll handler.
pub fn discard_dismissed(state: ViewerState) {
    if !state.search.dismissed.get_untracked() || JUST_DISMISSED.with(Cell::get) {
        return;
    }
    state.search.query.set(String::new());
    clear_search(state);
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

/// Attribute marking the search bar and the toolbar button that opens it.
/// Interacting with either is "coming back to the search", not "moving on".
///
/// An attribute rather than an id because several elements carry it, and
/// duplicate ids are invalid.
pub const SEARCH_CHROME_ATTR: &str = "data-search-chrome";

/// Whether `node` sits inside the search chrome.
fn in_search_chrome(node: &web_sys::Node) -> bool {
    // `closest` already walks ancestors; a text node has to be lifted to its
    // parent element first because only Elements implement it.
    node.dyn_ref::<web_sys::Element>()
        .cloned()
        .or_else(|| node.parent_element())
        .and_then(|el| el.closest(&format!("[{SEARCH_CHROME_ATTR}]")).ok().flatten())
        .is_some()
}

/// Whether a keypress means "the reader moved on", ending the grace period.
///
/// Three kinds of key do NOT count. Escape, so a second Escape (closing the
/// sidebar) does not also wipe the trace. Cmd/Ctrl+F, which reopens the bar.
/// And a bare modifier: it is the first half of a chord, and Cmd/Ctrl+F arrives
/// as TWO keydowns ("Control", then "f") — treating the modifier as an
/// interaction wiped the trace before the reopening keystroke was ever seen.
fn key_ends_grace(key: &str, cmd_or_ctrl: bool) -> bool {
    let bare_modifier = matches!(key, "Control" | "Shift" | "Alt" | "Meta");
    let reopening = cmd_or_ctrl && key.eq_ignore_ascii_case("f");
    key != "Escape" && !bare_modifier && !reopening
}

/// Ends the grace period after the reader dismisses a search.
///
/// While a dismissed search is pending, the next scroll, pointerdown or keypress
/// discards it: the muted highlights disappear and the query is emptied. Until
/// then everything is still held, so reopening the bar resumes it intact.
///
/// Interactions with the search chrome do NOT count — reopening the bar is how
/// the reader comes BACK to the search, not how they move on.
///
/// Each handler begins with an untracked bool read, so a scroll with no pending
/// search costs one signal read rather than a DOM walk.
///
/// Must be called once from the app root (ReaderView).
pub fn dismissed_search_watch(state: ViewerState) {
    // `scroll` does not bubble from a scrolling element to the window, so it
    // must be observed in the CAPTURE phase. Leptos's typed helper only
    // attaches bubble-phase listeners, hence the manual closure.
    //
    // The Closure is !Send, so it is parked in a local StoredValue that lives
    // as long as this owner (the same pattern as util::dom::observe_content_size)
    // while `on_cleanup` — which requires Send + Sync — detaches by the
    // independent JS Function handle.
    let scroll_closure: ScrollClosureSlot =
        StoredValue::new_local(None);
    let closure = Closure::wrap(Box::new(move |_: web_sys::Event| {
        discard_dismissed(state);
    }) as Box<dyn Fn(web_sys::Event)>);
    let js_fn: js_sys::Function = closure.as_ref().unchecked_ref::<js_sys::Function>().clone();
    if let Some(window) = web_sys::window() {
        _ = window.add_event_listener_with_callback_and_bool("scroll", &js_fn, true);
    }
    scroll_closure.set_value(Some(closure));
    on_cleanup(move || {
        if let Some(window) = web_sys::window() {
            _ = window.remove_event_listener_with_callback_and_bool("scroll", &js_fn, true);
        }
    });

    let pointer_handle =
        window_event_listener(leptos::ev::pointerdown, move |ev: leptos::ev::PointerEvent| {
            if !state.search.dismissed.get_untracked() {
                return;
            }
            let target: web_sys::Node = event_target(&ev);
            if !in_search_chrome(&target) {
                discard_dismissed(state);
            }
        });
    on_cleanup(move || pointer_handle.remove());

    // Any key that is not the reader reaching back for the search. Escape is
    // excluded so a second Escape (closing the sidebar) does not also wipe the
    // trace; Cmd/Ctrl+F is excluded because it reopens the bar.
    let key_handle =
        window_event_listener(leptos::ev::keydown, move |ev: leptos::ev::KeyboardEvent| {
            if !state.search.dismissed.get_untracked() {
                return;
            }
            if key_ends_grace(&ev.key(), ev.meta_key() || ev.ctrl_key()) {
                discard_dismissed(state);
            }
        });
    on_cleanup(move || key_handle.remove());
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
