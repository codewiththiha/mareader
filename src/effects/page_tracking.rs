//! Page-tracking: keeps `viewer.page` and the continuous scroll position in
//! sync so the status-bar counter follows scrolling and page jumps (nav,
//! thumbnails, outline, search) actually move the scrollport in continuous
//! mode.
//!
//! Wired once from ReaderView (like `fit_effect`). All effects are no-ops
//! outside continuous mode:
//!   1. mode-flip — entering continuous mode aligns `scroll_top` to the current
//!      page. `viewer.page` is the source of truth here, NOT the stale offset
//!      left over from a previous continuous session.
//!   2. scroll->page — scrolling derives the page at the top of the scrollport
//!      and writes `viewer.page`. This also reacts to height changes (zoom,
//!      lazy render fills), so the counter tracks whichever page the current
//!      offset now shows.
//!   3. page->scroll — an explicit page jump scrolls the `scroll_top` signal AND
//!      the actual `#page-list` DOM to the page top when they disagree, rounded
//!      UP so a fractional page top is crossed (an `as i32` floor would land
//!      short and the counter would read the previous page). If page heights
//!      aren't measured yet (first continuous session), it falls back to the
//!      same uniform estimate PageList uses to seed heights.
//!   4. exact-landing — the effect-3 target is placeholder-based and can drift
//!      for documents with varied page sizes, and a layout change (e.g. closing
//!      the sidebar mid-jump) can move the page while we're landing on it. This
//!      re-aims the scroll at the target page until scroll matches its exact
//!      rendered top (`cont-{P-1}-wrap` offsetTop), riding out both drift and
//!      the scale race. While the wrapper isn't mounted yet it aims at a fresh
//!      height-estimate so the page comes into view and mounts. It abandons only
//!      when the USER scrolls — the scroll signal diverging from the last value
//!      effects 3/4 wrote — never on distance alone.
//!
//! The two one-way syncs share a suppression flag: effect 2 sets it just before
//! writing `page`, effect 3 clears it on its next run. That is what stops them
//! from ping-ponging — effect 2 only fires on scroll/height changes, effect 3
//! only on page/mode changes, so the dependency sets never form a cycle.
//!
//! CRITICAL Leptos gotcha: effects only subscribe to signals they READ during a
//! run, so effects 3/4 read `page`/`mode`/`scroll_top`/`heights` unconditionally
//! at the top — a conditional read would silently drop a subscription the first
//! time a branch was skipped, and the effect would never fire again.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::core::layout::{page_from_scroll, page_top_css, ViewMode, PAGE_GAP};
use crate::core::state::AppState;

/// Uniform page height used when real heights haven't been measured yet — the
/// same placeholder PageList seeds `page_heights` with.
fn estimated_top(page: u32, state: AppState) -> f64 {
    let est = state
        .doc
        .page1_size
        .get_untracked()
        .map(|s| s.height)
        .unwrap_or(0.0)
        * state.viewer.render_scale.get_untracked();
    (page.saturating_sub(1)) as f64 * (est + PAGE_GAP)
}

/// Must be called once from the app root (ReaderView), alongside `fit_effect`.
pub fn page_tracking(state: AppState) {
    // --- 1. Entering continuous mode: align scroll to the current page -------
    // `page_heights`/`page1_size` are read untracked so this effect only fires
    // on a real mode transition (not on every render/zoom).
    let mut was_continuous = state.viewer.mode.get_untracked() == ViewMode::Continuous;
    Effect::new(move || {
        let continuous = state.viewer.mode.get() == ViewMode::Continuous;
        if continuous && !was_continuous {
            let page = state.viewer.page.get_untracked();
            let heights = state.doc.page_heights.get_untracked();
            let top = if heights.is_empty() {
                estimated_top(page, state)
            } else {
                page_top_css(page.saturating_sub(1) as usize, &heights, PAGE_GAP)
            };
            state.viewer.scroll_top.set(top);
        }
        was_continuous = continuous;
    });

    // Shared suppression flag (see module docs). Non-reactive on purpose.
    let suppress = Rc::new(Cell::new(false));

    // --- 2. scroll -> page ---------------------------------------------------
    let mode = state.viewer.mode;
    let page = state.viewer.page;
    let scroll_top = state.viewer.scroll_top;
    let heights = state.doc.page_heights;
    let suppress_a = suppress.clone();
    Effect::new(move || {
        if mode.get() != ViewMode::Continuous {
            return;
        }
        let st = scroll_top.get();
        let hs = heights.get();
        if hs.is_empty() {
            return;
        }
        let p = page_from_scroll(st, &hs, PAGE_GAP);
        if page.get_untracked() != p {
            suppress_a.set(true);
            page.set(p);
        }
    });

    // --- 3. page -> scroll ---------------------------------------------------
    // Two-phase: immediately scroll to the height-estimate target rounded UP
    // (so a fractional page top is crossed and the counter reads the target
    // page), then record the page as "pending" for effect 4 to snap to the
    // exact rendered position once its wrapper mounts.
    let suppress_b = suppress;
    // Shared by effects 3/4, non-reactive on purpose:
    //   pending   — the page we're trying to land on (None = no jump in flight).
    //   last_ours — the last scroll value WE wrote (jump or snap). If the
    //               scroll signal later differs from it, the USER took over and
    //               we abandon. During a layout/scale race the scroll stays put
    //               while the target moves, so we keep re-aiming.
    let pending = Rc::new(Cell::new(None::<u32>));
    let last_ours = Rc::new(Cell::new(f64::NAN));
    let pending_e3 = pending.clone();
    let last_ours_e3 = last_ours.clone();
    Effect::new(move || {
        // `page`/`mode` are read unconditionally (see module docs).
        let p = page.get();
        let continuous = mode.get() == ViewMode::Continuous;
        if !continuous {
            return;
        }
        if suppress_b.get() {
            suppress_b.set(false);
            return;
        }
        let hs = heights.get_untracked();
        let target_top = if hs.is_empty() {
            estimated_top(p, state)
        } else {
            page_top_css(p.saturating_sub(1) as usize, &hs, PAGE_GAP)
        };
        // Round UP: the boundary is crossed exactly even for fractional page
        // tops — `as i32` truncation (floor) would land short.
        let target_px = target_top.ceil();
        // Align the scroll signal (drives the visible-page window)...
        if hs.is_empty() || page_from_scroll(scroll_top.get_untracked(), &hs, PAGE_GAP) != p {
            scroll_top.set(target_px);
        }
        // ...and the real scrollport.
        if let Some(list) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("page-list"))
        {
            if hs.is_empty() || page_from_scroll(list.scroll_top() as f64, &hs, PAGE_GAP) != p {
                let _ = list.set_scroll_top(target_px as i32);
            }
        }
        // The target wrapper may not be mounted yet for a far jump; effect 4
        // re-syncs to the exact position once it is.
        pending_e3.set(Some(p));
        last_ours_e3.set(target_px);
    });

    // --- 4. exact-landing correction -----------------------------------------
    // Active only while a jump is pending (effect 3 set `pending`). Re-fires on
    // every scroll/height change — exactly when the layout shifts under us
    // (e.g. closing the sidebar mid-jump changes scale and moves every page) or
    // when the target wrapper mounts. Re-aims the scroll at the target page: at
    // the wrapper's exact offsetTop once it's mounted, else at a fresh
    // height-estimate so the page comes into view and mounts. It abandons only
    // when the USER scrolls — the scroll signal diverging from the last value we
    // wrote — never on distance, so a scale race can't abort the correction.
    let pending_b = pending;
    let last_ours_b = last_ours;
    Effect::new(move || {
        // Read deps unconditionally at the top (see module docs).
        let _ = mode.get();
        let _ = scroll_top.get();
        let hs = heights.get();
        if mode.get() != ViewMode::Continuous {
            pending_b.set(None);
            return;
        }
        let Some(target) = pending_b.get() else {
            return;
        };
        // User takeover: the scroll signal moved to something we didn't write.
        let mine = last_ours_b.get();
        if !mine.is_nan() && (scroll_top.get() - mine).abs() >= 0.5 {
            pending_b.set(None);
            return;
        }
        let Some(list) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("page-list"))
        else {
            return;
        };
        let cur = list.scroll_top() as f64;
        // The positioned wrapper's offsetTop is the page's EXACT rendered top
        // (page_top_css(i) with real heights). Only HtmlElement has offset_top.
        let wrap_id = format!("cont-{}-wrap", target.saturating_sub(1));
        let exact_top = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(&wrap_id))
            .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
            .map(|el| el.offset_top() as f64);
        match exact_top {
            Some(top) => {
                if (top - cur).abs() >= 0.5 {
                    scroll_top.set(top);
                    last_ours_b.set(top);
                    let _ = list.set_scroll_top(top as i32);
                }
                // Landed exactly. Deliberately do NOT clear pending here: a jump
                // closes the sidebar in the same gesture, and the resulting
                // resize/scale change moves the target page AFTER we land. Keeping
                // pending armed lets us re-aim at the moving page until the layout
                // settles. It is cleared on user takeover, mode exit, or the next
                // jump — never on a momentary exact match.
            }
            None => {
                // Wrapper not mounted yet — aim at the current height-estimate so
                // the page enters the render window and mounts. Never clear here.
                let est = if hs.is_empty() {
                    estimated_top(target, state)
                } else {
                    page_top_css(target.saturating_sub(1) as usize, &hs, PAGE_GAP)
                }
                .ceil();
                if (est - cur).abs() >= 0.5 {
                    scroll_top.set(est);
                    last_ours_b.set(est);
                    let _ = list.set_scroll_top(est as i32);
                }
            }
        }
    });
}
