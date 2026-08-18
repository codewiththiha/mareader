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
//!   2. scroll->page — scrolling derives the DOMINANT page (the one filling
//!      most of the viewport) and writes `viewer.page`. This also reacts to
//!      height changes (zoom, lazy render fills), so the counter tracks
//!      whichever page the current offset now shows. It deliberately does NOT
//!      use the top-edge page: zooming out slides more of the previous page
//!      into the top of the viewport, which walked the counter (1 -> 2 -> 3...)
//!      while the reader was in fact holding perfectly still.
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
use std::time::Duration;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use pdf_core::layout::{dominant_page, page_top_css, ViewMode, PAGE_GAP};
use crate::state::ViewerState;
use crate::dom::page_list;

/// How long a smooth jump is allowed to be in flight. The browser owns the
/// animation and doesn't tell us when it finishes, so effect 4's takeover
/// detection is gated for this long: during a smooth scroll the offset moves
/// on its own, which looks exactly like the user grabbing the scrollbar. Long
/// enough to cover a ~300ms native smooth scroll with margin.
const JUMP_SETTLE_MS: f64 = 450.0;

/// Scroll `#page-list` to `top`, smoothly for nearby targets and instantly for
/// far ones.
///
/// Preview glides between adjacent pages but cuts straight to a distant jump —
/// a smooth scroll across fifty pages is a long blur through content nobody
/// asked to see, and it thrashes the virtualizer mounting and unmounting every
/// page on the way. The `2 * viewport` threshold is the same one the thumbnail
/// panel's glide uses, so navigation feels consistent in both places.
fn scroll_to(list: &web_sys::Element, top: f64, smooth: bool) {
    let opts = web_sys::ScrollToOptions::new();
    opts.set_top(top);
    opts.set_behavior(if smooth {
        web_sys::ScrollBehavior::Smooth
    } else {
        web_sys::ScrollBehavior::Instant
    });
    list.scroll_to_with_scroll_to_options(&opts);
}

/// Uniform page height used when real heights haven't been measured yet — the
/// same placeholder PageList seeds `page_heights` with.
fn estimated_top(page: u32, state: ViewerState) -> f64 {
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
pub fn page_tracking(state: ViewerState) {
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
        // Read the scrollport's real height; `container_size` is tracked too so
        // this re-runs when the viewer is resized.
        let (_, cont_h) = state.viewer.container_size.get();
        let vh = page_list()
            .map(|el| el.client_height() as f64)
            .filter(|h| *h > 1.0)
            .unwrap_or(cont_h);
        let p = dominant_page(st, vh, &hs, PAGE_GAP);
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
    // Timestamp until which a smooth jump owns the scrollport (0 = none).
    let jump_settle = Rc::new(Cell::new(0.0f64));
    // Wakes effect 4 once the settle gate lapses.
    let settle_timer: StoredValue<Option<TimeoutHandle>> = StoredValue::new(None);
    let pending_e3 = pending.clone();
    let last_ours_e3 = last_ours.clone();
    let jump_settle_e3 = jump_settle.clone();
    let jump_settle_e4 = jump_settle;
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
        // Compared with the SAME metric effect 2 uses, or the two would
        // disagree about whether we have arrived and fight each other.
        let vh_now = page_list()
            .map(|el| el.client_height() as f64)
            .filter(|h| *h > 1.0)
            .unwrap_or_else(|| state.viewer.container_size.get_untracked().1);
        if hs.is_empty() || dominant_page(scroll_top.get_untracked(), vh_now, &hs, PAGE_GAP) != p {
            scroll_top.set(target_px);
        }
        // ...and the real scrollport.
        if let Some(list) = page_list()
            && (hs.is_empty() || dominant_page(list.scroll_top() as f64, vh_now, &hs, PAGE_GAP) != p)
        {
            let cur = list.scroll_top() as f64;
            let vh = list.client_height() as f64;
            // Nearby (a page turn) glides; far (outline/thumb/search jump)
            // cuts. See `scroll_to`.
            let smooth = vh > 0.0 && (target_px - cur).abs() <= 2.0 * vh;
            scroll_to(&list, target_px, smooth);
            if smooth {
                // Arm the settle gate: a smooth scroll moves the offset for
                // the next few hundred ms, and effect 4 must not read that
                // motion as the user taking over and abandon the landing.
                jump_settle_e3.set(js_sys::Date::now() + JUMP_SETTLE_MS);
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
        mode.get();
        scroll_top.get();
        let hs = heights.get();
        if mode.get() != ViewMode::Continuous {
            pending_b.set(None);
            return;
        }
        let Some(target) = pending_b.get() else {
            return;
        };
        // A smooth jump is still gliding: the offset is moving under browser
        // control, which is indistinguishable from a user drag by value alone.
        // Hold off entirely until it settles, then do ONE instant correction if
        // it mis-landed (heights can change mid-glide as pages render in).
        let settling = js_sys::Date::now() < jump_settle_e4.get();
        if settling {
            // Re-check after the gate lapses; without this the correction would
            // only run if some other signal happened to fire again.
            let remain = (jump_settle_e4.get() - js_sys::Date::now()).max(0.0);
            if let Some(h) = settle_timer.get_value() {
                h.clear();
            }
            let bump = scroll_top;
            settle_timer.set_value(
                set_timeout_with_handle(
                    move || bump.update(|v| *v += 0.0),
                    Duration::from_millis(remain as u64 + 30),
                )
                .ok(),
            );
            return;
        }

        // User takeover: the scroll signal moved to something we didn't write.
        let mine = last_ours_b.get();
        if !mine.is_nan() && (scroll_top.get() - mine).abs() >= 0.5 {
            pending_b.set(None);
            return;
        }
        let Some(list) = page_list()
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
                    list.set_scroll_top(top as i32);
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
                    list.set_scroll_top(est as i32);
                }
            }
        }
    });
}
