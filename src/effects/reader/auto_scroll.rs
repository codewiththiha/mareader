//! Continuous auto-scroll along the active strip (vertical or horizontal).
//!
//! The drift is a [`FrameLoop`]: one frame at a time, each frame deciding
//! whether it needs another. It used to be a free `fn tick` that re-armed
//! itself with its state parked in two `thread_local!` cells — a running flag
//! and the previous frame's stamp. That worked, and it was the only loop in the
//! app that could not be stopped from the outside: nothing owned it, so a
//! document closed mid-drift left a queued frame callback reading signals whose
//! reactive graph had already been disposed. The loop's own flag check could
//! not help, because the flag lived in a thread-local that outlived the reader
//! it belonged to, and the read that panicked was the one before it.

use leptos::prelude::*;

use app_chrome::hooks::dom::{h_page_list, page_list};
use app_chrome::hooks::use_raf::FrameLoop;

use crate::components::primitives::motion::frame::{MAX_SCROLL_FRAME_S, frame_delta};
use crate::state::ReaderState;

const AUTO_SCROLL_PX_PER_SEC: f64 = 72.0;

pub fn auto_scroll(state: ReaderState) {
    // Paginated modes can't scroll: force the toggle off so the menu row
    // never shows an active-but-dead option.
    Effect::new(move |_| {
        if state.viewer.auto_scroll.get() && !state.viewer.mode.get().can_scroll() {
            state.viewer.auto_scroll.set(false);
        }
    });

    // The previous frame's stamp belongs to ONE drift: it is reset when the
    // toggle goes on, so the first frame of a new drift passes no time and the
    // second starts moving at a real rate.
    let last_ms = StoredValue::new_local(f64::NAN);
    let frames = FrameLoop::new();
    Effect::new(move |_| {
        if !state.viewer.auto_scroll.get() {
            frames.stop();
            return;
        }
        last_ms.set_value(f64::NAN);
        frames.arm(move || tick(state, last_ms));
    });
}

/// One frame of drift. `false` ends the loop: the toggle went off, the mode
/// stopped being scrollable, or the strip ran out.
fn tick(state: ReaderState, last_ms: StoredValue<f64, LocalStorage>) -> bool {
    let mode = state.viewer.mode.get_untracked();
    if !state.viewer.auto_scroll.get_untracked() || !mode.can_scroll() {
        state.viewer.auto_scroll.set(false);
        return false;
    }
    let now = js_sys::Date::now();
    // A frame after the tab was backgrounded reports seconds of gap; clamped,
    // so returning to the window resumes the drift instead of leaping.
    let dt = frame_delta(last_ms.get_value(), now, MAX_SCROLL_FRAME_S);
    last_ms.set_value(now);
    let delta = AUTO_SCROLL_PX_PER_SEC * dt;

    let done = match mode {
        reader_core::view::ViewMode::ScrollVertical => page_list()
            .map(|el| {
                step(
                    &el,
                    delta,
                    el.scroll_height(),
                    el.client_height(),
                    el.scroll_top(),
                    |e, v| e.set_scroll_top(v),
                )
            })
            .unwrap_or(false),
        reader_core::view::ViewMode::ScrollHorizontal => h_page_list()
            .map(|el| {
                step(
                    &el,
                    delta,
                    el.scroll_width(),
                    el.client_width(),
                    el.scroll_left(),
                    |e, v| e.set_scroll_left(v),
                )
            })
            .unwrap_or(false),
        _ => false,
    };
    if done {
        state.viewer.auto_scroll.set(false);
        return false;
    }
    true
}

/// Returns true when the end of the strip was reached.
fn step(
    el: &web_sys::Element,
    delta: f64,
    total: i32,
    client: i32,
    cur: i32,
    set: impl Fn(&web_sys::Element, i32),
) -> bool {
    let max = (total - client).max(0);
    let next = (cur + delta.round() as i32).min(max);
    set(el, next);
    next >= max
}

pub fn handle_auto_scroll_shortcut(state: ReaderState, ev: &leptos::ev::KeyboardEvent) {
    if ev.shift_key() && ev.key().to_lowercase() == "a" {
        ev.prevent_default();
        if state.viewer.mode.get().can_scroll() {
            let on = state.viewer.auto_scroll.get();
            state.viewer.auto_scroll.set(!on);
        }
    }
}
