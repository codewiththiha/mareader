//! Page navigation and the continuous-scroll hold engine.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use std::cell::{Cell, RefCell};

use crate::pane::dom::PaneDom;
use crate::state::ReaderState;
use app_ui::components::primitives::motion::frame::{MAX_SCROLL_FRAME_S, frame_delta};
use reader_core::view::{ViewMode, spread_step_next, spread_step_prev};

use super::is_chrome_scroll_target;
use super::keymap::{self, NavAction};

/// One Arrow tap is a reading nudge, not a page jump.
fn line_scroll_px(viewport_h: f64) -> f64 {
    (viewport_h * 0.08).clamp(40.0, 80.0)
}

/// PageUp / PageDown / Space: almost a screen, with an overlap
/// sliver.
fn page_scroll_px(viewport_h: f64) -> f64 {
    (viewport_h * 0.9).max(1.0)
}

/// Delay before a held arrow repeats, then a continuous glide.
const HOLD_DELAY_MS: f64 = 350.0;
const HOLD_PX_PER_SEC: f64 = 1000.0;

// thread_local, not StoredValue: the window listeners share no
// reactive owner.
thread_local! {
    static HOLD_DIR: Cell<f64> = const { Cell::new(0.0) };
    static HOLD_DOWN_AT: Cell<f64> = const { Cell::new(0.0) };
    static HOLD_LAST: Cell<f64> = const { Cell::new(0.0) };
    static HOLD_RAF: Cell<bool> = const { Cell::new(false) };
    /// 1 = vertical (#page-list), 2 = horizontal (#h-page-list).
    static HOLD_AXIS: Cell<u8> = const { Cell::new(1) };
    /// The strip the running hold scrolls, captured at key-down.
    static HOLD_TARGET: RefCell<Option<web_sys::Element>> = const { RefCell::new(None) };
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

/// Keep focus on the strip itself, not a span about to unmount.
fn focus_scroll_list(dom: PaneDom, horizontal: bool) {
    let Some(list) = strip(dom, horizontal) else {
        return;
    };
    let Some(html) = list.dyn_ref::<web_sys::HtmlElement>() else {
        return;
    };
    let opts = web_sys::FocusOptions::new();
    opts.set_prevent_scroll(true);
    _ = html.focus_with_options(&opts);
}

/// Scroll a strip by `delta`, clamped and skipped at the edge.
fn scroll_reader_axis(list: &web_sys::Element, horizontal: bool, delta: f64, smooth: bool) {
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

/// The active pane's strip on one axis, found inside the pane's own root.
fn strip(dom: PaneDom, horizontal: bool) -> Option<web_sys::Element> {
    if horizontal {
        dom.h_page_list()
    } else {
        dom.page_list()
    }
}

/// The strip's viewport length along its main axis.
fn viewport_len(list: &web_sys::Element, horizontal: bool) -> f64 {
    if horizontal {
        list.client_width() as f64
    } else {
        list.client_height() as f64
    }
}

/// One line-sized nudge of the pane's strip.
fn scroll_reader_line(dom: PaneDom, horizontal: bool, dir: f64, smooth: bool) {
    let Some(list) = strip(dom, horizontal) else {
        return;
    };
    let delta = dir * line_scroll_px(viewport_len(&list, horizontal));
    scroll_reader_axis(&list, horizontal, delta, smooth);
}

/// One screen-sized step of the pane's strip.
fn scroll_reader_page(dom: PaneDom, horizontal: bool, dir: f64, smooth: bool) {
    let Some(list) = strip(dom, horizontal) else {
        return;
    };
    let delta = dir * page_scroll_px(viewport_len(&list, horizontal));
    scroll_reader_axis(&list, horizontal, delta, smooth);
}

fn begin_line_hold(dom: PaneDom, dir: f64, horizontal: bool, glide: bool) {
    HOLD_DIR.with(|d| d.set(dir));
    HOLD_TARGET.with(|t| *t.borrow_mut() = strip(dom, horizontal));
    HOLD_AXIS.with(|a| a.set(if horizontal { 2 } else { 1 }));
    let now = js_sys::Date::now();
    HOLD_DOWN_AT.with(|t| t.set(now));
    HOLD_LAST.with(|t| t.set(now));
    // A tap is an ANIMATION, so `glide` decides; the hold is not.
    focus_scroll_list(dom, horizontal);
    scroll_reader_line(dom, horizontal, dir, glide);
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

/// Stops the glide: focus lost, or the pane is going away.
pub(super) fn stop_hold() {
    HOLD_DIR.with(|d| d.set(0.0));
    HOLD_TARGET.with(|t| t.borrow_mut().take());
}

fn hold_tick() {
    let dir = HOLD_DIR.with(|d| d.get());
    if dir == 0.0 {
        HOLD_RAF.with(|r| r.set(false));
        // The hold is over: the strip it captured is released with it.
        HOLD_TARGET.with(|t| t.borrow_mut().take());
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
        let dt = frame_delta(last, now, MAX_SCROLL_FRAME_S);
        let delta = dir * HOLD_PX_PER_SEC * dt;
        let horizontal = HOLD_AXIS.with(|a| a.get()) == 2;
        HOLD_TARGET.with(|t| {
            if let Some(list) = t.borrow().as_ref() {
                scroll_reader_axis(list, horizontal, delta, false);
            }
        });
    }
    request_animation_frame(hold_tick);
}

/// The plain-key arms: arrows, PageUp/Down and Space.
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
    // Untracked: the switch that turns the glide off cannot trigger one.
    let glide = state.viewer.motion.get_untracked().scroll_glide;
    match action {
        NavAction::PagePrev => page_prev(state),
        NavAction::PageNext => page_next(state),
        NavAction::HoldLine { dir, horizontal } => {
            begin_line_hold(state.dom, dir as f64, horizontal, glide)
        }
        NavAction::PageStep { dir, horizontal } => {
            focus_scroll_list(state.dom, horizontal);
            // A repeat would queue a stack of overlapping smooth scrolls.
            let smooth = glide && !ev.repeat();
            scroll_reader_page(state.dom, horizontal, dir as f64, smooth);
        }
    }
}

/// Ends the rAF glide on keyup, vim aliases included.
pub(super) fn end_hold_for(key: &str) {
    match keymap::arrow_key(key).unwrap_or(key) {
        "ArrowUp" | "ArrowLeft" => end_line_hold(-1.0),
        "ArrowDown" | "ArrowRight" => end_line_hold(1.0),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{line_scroll_px, page_scroll_px};

    #[test]
    fn a_line_step_is_a_reading_nudge_not_a_page_jump() {
        // A 900px viewer used to jump 135px per key, like paging.
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
