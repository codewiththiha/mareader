//! Page navigation and the continuous-scroll hold engine: arrows turn pages
//! in single/dual mode and glide the scrollport in continuous/horizontal mode;
//! PageUp/Down and Space page the column.
//!
//! What a key MEANS is not decided here — that is [`super::keymap`], which is
//! pure and covered by tests. This file is the doing: the scroll helpers, the
//! rAF hold engine behind a held arrow, and the small bridge that reads an
//! event into the keymap's inputs and performs its answer.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use std::cell::Cell;

use pdf_core::layout::{ViewMode, spread_step_next, spread_step_prev};
use crate::components::primitives::hooks::dom::{h_page_list, page_list};
use crate::state::ReaderState;

use super::is_chrome_scroll_target;
use super::keymap::{self, NavAction};

/// One Arrow Up/Down tap is a reading nudge, not a page jump.
///
/// The first owner-scroll used 15% of the viewport (48–140px). Native
/// browser line-scroll is ~40px, so a hold felt like paging: each
/// key-repeat teleported a sixth of the screen with no glide. 8%
/// clamped to a native-ish band matches the old feel without giving
/// the keys back to a text-layer span that virtualization will unmount.
pub(crate) fn line_scroll_px(viewport_h: f64) -> f64 {
    (viewport_h * 0.08).clamp(40.0, 80.0)
}

/// PageUp / PageDown / Space: almost a screen, with a sliver of overlap
/// so the reader does not lose the last line they just saw.
pub(crate) fn page_scroll_px(viewport_h: f64) -> f64 {
    (viewport_h * 0.9).max(1.0)
}

/// Native-like delay before a held arrow starts repeating, then a
/// continuous glide (px/s) instead of discrete jumps. 350ms sits
/// between macOS (~250) and Windows (~500). 1000 px/s is roughly a
/// viewport a second — reading speed, not a flick.
const HOLD_DELAY_MS: f64 = 350.0;
const HOLD_PX_PER_SEC: f64 = 1000.0;

// thread_local, not StoredValue: the hold engine is driven from window
// keydown/keyup listeners that do not share a reactive owner, so the
// rAF loop has to outlive any one effect.
thread_local! {
    static HOLD_DIR: Cell<f64> = const { Cell::new(0.0) };
    static HOLD_DOWN_AT: Cell<f64> = const { Cell::new(0.0) };
    static HOLD_LAST: Cell<f64> = const { Cell::new(0.0) };
    static HOLD_RAF: Cell<bool> = const { Cell::new(false) };
    /// 1 = vertical (#page-list), 2 = horizontal (#h-page-list).
    static HOLD_AXIS: Cell<u8> = const { Cell::new(1) };
}

fn page_prev(state: ReaderState) {
    if state.viewer.mode.get() == ViewMode::Spread {
        state
            .viewer
            .page
            .set(spread_step_prev(state.viewer.page.get()));
    } else if state.viewer.page.get() > 1 {
        state.viewer.page.set(state.viewer.page.get() - 1);
    }
}

fn page_next(state: ReaderState) {
    let n = state.document.num_pages.get();
    if state.viewer.mode.get() == ViewMode::Spread {
        state
            .viewer
            .page
            .set(spread_step_next(n, state.viewer.page.get()));
    } else if n > 0 && state.viewer.page.get() < n {
        state.viewer.page.set(state.viewer.page.get() + 1);
    }
}

/// Keep keyboard focus on the active scroll strip itself (not a text-layer
/// span the virtualizer is about to unmount). `preventScroll` so focusing
/// does not fight the scroll we are about to apply.
fn focus_scroll_list(horizontal: bool) {
    let Some(list) = (if horizontal { h_page_list() } else { page_list() }) else {
        return;
    };
    let Some(html) = list.dyn_ref::<web_sys::HtmlElement>() else {
        return;
    };
    let opts = web_sys::FocusOptions::new();
    opts.set_prevent_scroll(true);
    _ = html.focus_with_options(&opts);
}

/// Scroll one of the reader's strips by `delta` along its main axis, clamped
/// to the scrollable range and skipped entirely when the clamp eats the step
/// (a boundary hold must not fight the elastic edge). The y/x twins differ
/// only in element and axis properties, so one helper serves both.
fn scroll_reader_axis(horizontal: bool, delta: f64, smooth: bool) {
    let Some(list) = (if horizontal { h_page_list() } else { page_list() }) else {
        return;
    };
    let (current, extent, client) = if horizontal {
        (
            list.scroll_left() as f64,
            list.scroll_width() as f64,
            list.client_width() as f64,
        )
    } else {
        (
            list.scroll_top() as f64,
            list.scroll_height() as f64,
            list.client_height() as f64,
        )
    };
    let max = (extent - client).max(0.0);
    let next = (current + delta).clamp(0.0, max);
    if (next - current).abs() < 0.5 {
        return;
    }
    let opts = web_sys::ScrollToOptions::new();
    if horizontal {
        opts.set_left(next);
    } else {
        opts.set_top(next);
    }
    opts.set_behavior(if smooth {
        web_sys::ScrollBehavior::Smooth
    } else {
        web_sys::ScrollBehavior::Instant
    });
    list.scroll_to_with_scroll_to_options(&opts);
}

fn scroll_reader_y(dy: f64, smooth: bool) {
    scroll_reader_axis(false, dy, smooth);
}

fn scroll_reader_x(dx: f64, smooth: bool) {
    scroll_reader_axis(true, dx, smooth);
}

fn scroll_reader_line_y(dir: f64, smooth: bool) {
    let Some(list) = page_list() else { return };
    scroll_reader_y(dir * line_scroll_px(list.client_height() as f64), smooth);
}

fn scroll_reader_page_y(dir: f64, smooth: bool) {
    let Some(list) = page_list() else { return };
    scroll_reader_y(dir * page_scroll_px(list.client_height() as f64), smooth);
}

fn scroll_reader_line_x(dir: f64, smooth: bool) {
    let Some(list) = h_page_list() else { return };
    scroll_reader_x(dir * line_scroll_px(list.client_width() as f64), smooth);
}

fn scroll_reader_page_x(dir: f64, smooth: bool) {
    let Some(list) = h_page_list() else { return };
    scroll_reader_x(dir * page_scroll_px(list.client_width() as f64), smooth);
}

fn begin_line_hold(dir: f64, horizontal: bool, glide: bool) {
    HOLD_DIR.with(|d| d.set(dir));
    HOLD_AXIS.with(|a| a.set(if horizontal { 2 } else { 1 }));
    let now = js_sys::Date::now();
    HOLD_DOWN_AT.with(|t| t.set(now));
    HOLD_LAST.with(|t| t.set(now));
    // A tap is one smooth nudge — an ANIMATION, so `glide` (the reader's
    // scroll switch) decides whether it eases or lands. The hold that follows
    // is not: the rAF loop below IS the scrolling, frames and all, and it runs
    // whether or not the tap glided.
    focus_scroll_list(horizontal);
    if horizontal {
        scroll_reader_line_x(dir, glide);
    } else {
        scroll_reader_line_y(dir, glide);
    }
    if HOLD_RAF.with(|r| r.get()) {
        return;
    }
    HOLD_RAF.with(|r| r.set(true));
    request_animation_frame(hold_tick);
}

fn end_line_hold(dir: f64) {
    HOLD_DIR.with(|d| {
        if d.get() == dir {
            d.set(0.0);
        }
    });
}

fn stop_line_hold() {
    HOLD_DIR.with(|d| d.set(0.0));
}

fn hold_tick() {
    let dir = HOLD_DIR.with(|d| d.get());
    if dir == 0.0 {
        HOLD_RAF.with(|r| r.set(false));
        return;
    }
    let now = js_sys::Date::now();
    let last = HOLD_LAST.with(|t| {
        let prev = t.get();
        t.set(now);
        prev
    });
    let down_at = HOLD_DOWN_AT.with(|t| t.get());
    if now - down_at >= HOLD_DELAY_MS {
        let dt = ((now - last) / 1000.0).clamp(0.0, 0.05);
        let delta = dir * HOLD_PX_PER_SEC * dt;
        if HOLD_AXIS.with(|a| a.get()) == 2 {
            scroll_reader_x(delta, false);
        } else {
            scroll_reader_y(delta, false);
        }
    }
    request_animation_frame(hold_tick);
}

/// The plain-key navigation arms: arrows (page turn in single/dual mode,
/// scroll hold in continuous/horizontal), PageUp/Down and Space.
///
/// The DECISION lives in [`keymap::resolve`], which is pure and tested; this
/// reads the event into its inputs and performs the answer.
pub(super) fn handle_navigation_shortcut(state: ReaderState, ev: &leptos::ev::KeyboardEvent) {
    let key = ev.key();
    let outcome = keymap::resolve(keymap::NavKey {
        key: key.as_str(),
        shift: ev.shift_key(),
        repeat: ev.repeat(),
        mode: state.viewer.mode.get(),
        in_chrome: is_chrome_scroll_target(ev),
        on_button: ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::HtmlButtonElement>().ok())
            .is_some(),
    });

    if outcome.prevent_default {
        ev.prevent_default();
    }
    let Some(action) = outcome.action else {
        return;
    };
    // Whether this keypress may GLIDE anywhere. Untracked: the switch that
    // turns the glide off must not be what triggers one, and a keypress reads
    // the world as it is the moment it lands.
    let glide = state.viewer.motion.get_untracked().scroll_glide;
    match action {
        NavAction::PagePrev => page_prev(state),
        NavAction::PageNext => page_next(state),
        NavAction::HoldLine { dir, horizontal } => {
            begin_line_hold(dir as f64, horizontal, glide)
        }
        NavAction::PageStep { dir, horizontal } => {
            focus_scroll_list(horizontal);
            // A repeat is the browser hammering the key; easing each one
            // would queue a stack of overlapping smooth scrolls.
            let smooth = glide && !ev.repeat();
            if horizontal {
                scroll_reader_page_x(dir as f64, smooth);
            } else {
                scroll_reader_page_y(dir as f64, smooth);
            }
        }
    }
}

/// Ends the rAF glide on keyup; the entry dispatcher wires this.
pub(super) fn end_hold_for(key: &str) {
    match key {
        "ArrowUp" | "ArrowLeft" => end_line_hold(-1.0),
        "ArrowDown" | "ArrowRight" => end_line_hold(1.0),
        _ => {}
    }
}

/// Stops the glide when the window loses focus.
pub(super) fn stop_hold() {
    stop_line_hold();
}

#[cfg(test)]
mod tests {
    use super::{line_scroll_px, page_scroll_px};

    #[test]
    fn a_line_step_is_a_reading_nudge_not_a_page_jump() {
        // A 900px viewer used to jump 135px (15%) per key — three native
        // lines at once, which is what made arrows feel like they were
        // paging rather than scrolling.
        assert!((line_scroll_px(900.0) - 72.0).abs() < 0.01);
        assert_eq!(
            line_scroll_px(200.0),
            40.0,
            "never smaller than a native line"
        );
        assert_eq!(
            line_scroll_px(2000.0),
            80.0,
            "never a sixth of a tall window"
        );
        assert!(line_scroll_px(900.0) < page_scroll_px(900.0) / 4.0);
    }

    #[test]
    fn a_page_step_keeps_a_sliver_of_overlap() {
        assert!((page_scroll_px(800.0) - 720.0).abs() < 0.01);
        assert_eq!(page_scroll_px(0.0), 1.0);
    }
}
