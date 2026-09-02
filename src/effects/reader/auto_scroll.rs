//! Continuous auto-scroll along the active strip (vertical or horizontal).

use std::cell::Cell;

use leptos::prelude::*;

use app_chrome::hooks::dom::{h_page_list, page_list};
use crate::state::ReaderState;

const AUTO_SCROLL_PX_PER_SEC: f64 = 72.0;

thread_local! {
    static AS_RAF: Cell<bool> = const { Cell::new(false) };
    static AS_LAST: Cell<f64> = const { Cell::new(f64::NAN) };
}

pub fn auto_scroll(state: ReaderState) {
    // Paginated modes can't scroll: force the toggle off so the menu row
    // never shows an active-but-dead option.
    Effect::new(move |_| {
        if state.viewer.auto_scroll.get() && !state.viewer.mode.get().can_scroll() {
            state.viewer.auto_scroll.set(false);
        }
    });
    Effect::new(move |_| {
        if !state.viewer.auto_scroll.get() || AS_RAF.with(|r| r.get()) {
            return;
        }
        AS_RAF.with(|r| r.set(true));
        AS_LAST.with(|t| t.set(f64::NAN));
        request_animation_frame(move || tick(state));
    });
}

fn tick(state: ReaderState) {
    let mode = state.viewer.mode.get_untracked();
    if !state.viewer.auto_scroll.get_untracked() || !mode.can_scroll() {
        state.viewer.auto_scroll.set(false);
        AS_RAF.with(|r| r.set(false));
        return;
    }
    let now = js_sys::Date::now();
    let mut last = AS_LAST.with(|t| t.get());
    if last.is_nan() {
        last = now;
    }
    let dt = ((now - last) / 1000.0).clamp(0.0, 0.05);
    AS_LAST.with(|t| t.set(now));
    let delta = AUTO_SCROLL_PX_PER_SEC * dt;

    let done = match mode {
        pdf_core::layout::ViewMode::ScrollVertical => page_list()
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
        pdf_core::layout::ViewMode::ScrollHorizontal => h_page_list()
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
        AS_RAF.with(|r| r.set(false));
        return;
    }
    request_animation_frame(move || tick(state));
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
