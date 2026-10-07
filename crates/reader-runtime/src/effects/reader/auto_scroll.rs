//! Continuous auto-scroll along the active strip.

use leptos::prelude::*;

use app_chrome::hooks::use_raf::FrameLoop;

use crate::state::ReaderState;
use app_ui::components::primitives::motion::frame::{MAX_SCROLL_FRAME_S, frame_delta};

const AUTO_SCROLL_PX_PER_SEC: f64 = 72.0;

pub fn auto_scroll(state: ReaderState) {
    // Paginated modes cannot scroll; force the toggle off.
    Effect::new(move |_| {
        if state.viewer.auto_scroll.get() && !state.viewer.mode.get().can_scroll() {
            state.viewer.auto_scroll.set(false);
        }
    });

    // The frame stamp is reset when the toggle goes on.
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

/// One frame of drift; `false` ends the loop.
fn tick(state: ReaderState, last_ms: StoredValue<f64, LocalStorage>) -> bool {
    let mode = state.viewer.mode.get_untracked();
    if !state.viewer.auto_scroll.get_untracked() || !mode.can_scroll() {
        state.viewer.auto_scroll.set(false);
        return false;
    }
    let now = js_sys::Date::now();
    // A backgrounded tab reports a gap; clamp it.
    let dt = frame_delta(last_ms.get_value(), now, MAX_SCROLL_FRAME_S);
    last_ms.set_value(now);
    let delta = AUTO_SCROLL_PX_PER_SEC * dt;

    let done = match mode {
        reader_core::view::ViewMode::ScrollVertical => state
            .dom
            .page_list()
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
        reader_core::view::ViewMode::ScrollHorizontal => state
            .dom
            .h_page_list()
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
