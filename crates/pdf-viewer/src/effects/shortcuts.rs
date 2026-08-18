//! Global keyboard shortcuts. OWNED BY branch B (viewer/chrome).
//!
//! Must be called once from the app root (wired during integration).
//! The listener callback runs OUTSIDE the reactive owner, so everything it
//! touches is a Copy signal handle / ViewerState captured by value.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use std::cell::Cell;

use pdf_core::layout::ViewMode;
use pdf_core::math::{nearest_zoom, FitMode};
use crate::state::{ViewerState, SidebarMode};
use crate::effects::fit::request_zoom;
use crate::dom::page_list;

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

thread_local! {
    static HOLD_DIR: Cell<f64> = const { Cell::new(0.0) };
    static HOLD_DOWN_AT: Cell<f64> = const { Cell::new(0.0) };
    static HOLD_LAST: Cell<f64> = const { Cell::new(0.0) };
    static HOLD_RAF: Cell<bool> = const { Cell::new(false) };
}

/// Applies a manual zoom step and exits fit mode.
///
/// Steps from the zoom currently being aimed at, falling back to the displayed
/// scale when nothing is in flight. Both alternatives are wrong:
///   * `scale` still holds the PREVIOUS committed value during an animation,
///     so a fast `+ +` would resolve to the same preset twice and the second
///     press would do nothing.
///   * `display_scale` is a partway value mid-animation, so `nearest_zoom`
///     would usually return the preset it is already travelling towards —
///     again, a swallowed press.
///
/// Chaining from the in-flight target means each press advances exactly one
/// preset, while the coordinator retargets the running animation from wherever
/// it currently is rather than restarting it.
fn zoom_by(state: ViewerState, dir: i32) {
    let cur = state
        .viewer
        .zoom_request
        .get_untracked()
        .filter(|_| state.viewer.zoom_animating.get_untracked())
        .map(|(target, _, _)| target)
        .unwrap_or_else(|| state.viewer.display_scale.get_untracked());
    state.viewer.fit.set(FitMode::None);
    request_zoom(state, nearest_zoom(cur, dir), true);
}

fn page_prev(state: ViewerState) {
    let p = state.viewer.page.get();
    if p > 1 {
        state.viewer.page.set(p - 1);
    }
}

fn page_next(state: ViewerState) {
    let p = state.viewer.page.get();
    let n = state.doc.num_pages.get();
    if n > 0 && p < n {
        state.viewer.page.set(p + 1);
    }
}

/// Returns true when the keydown target is a form control (input / select),
/// where global shortcuts must not fire.
fn is_form_target(ev: &leptos::ev::KeyboardEvent) -> bool {
    ev.target().is_some_and(|target| {
        target.dyn_ref::<web_sys::HtmlInputElement>().is_some()
            || target.dyn_ref::<web_sys::HtmlSelectElement>().is_some()
    })
}

/// True when the key landed inside a chrome scroller (thumbs, outline, a
/// popover). Those own their own arrow keys; the reader must not steal them.
fn is_chrome_scroll_target(ev: &leptos::ev::KeyboardEvent) -> bool {
    let Some(el) = ev
        .target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
    else {
        return false;
    };
    for sel in [
        "#thumb-scroll",
        "aside",
        ".menu-popover",
        ".floating-search-enter",
    ] {
        if el.closest(sel).ok().flatten().is_some() {
            return true;
        }
    }
    false
}

/// Keep keyboard focus on `#page-list` itself (not a text-layer span
/// the virtualizer is about to unmount). `preventScroll` so focusing
/// does not fight the scroll we are about to apply.
fn focus_page_list() {
    let Some(list) = page_list() else { return };
    let Some(html) = list.dyn_ref::<web_sys::HtmlElement>() else {
        return;
    };
    let opts = web_sys::FocusOptions::new();
    opts.set_prevent_scroll(true);
    _ = html.focus_with_options(&opts);
}

/// Scroll `#page-list` by `dy` CSS px, clamped to the scrollable extent.
///
/// Arrow keys in continuous mode used to rely on the BROWSER scrolling
/// whatever had focus. Focus was almost always a text-layer span on the
/// page the reader had clicked. Virtualization unmounts that page a few
/// screens later, the focused node is removed, key-repeat dies, and the
/// next press lands on `<body>` which is not the scroll container — the
/// arrows "stop working" until the reader clicks the page again.
/// Owning the keys on `window` means they keep working regardless of
/// which node currently has focus.
///
/// A tap uses the same nearby-smooth path page jumps use (`ScrollBehavior
/// ::Smooth`) so one press glides instead of teleporting. A hold is a
/// rAF glide (see `begin_line_hold`) — assigning `scrollTop` on every
/// key-repeat was the chunky feel the reader lost.
fn scroll_reader(dy: f64, smooth: bool) {
    let Some(list) = page_list() else { return };
    let max = (list.scroll_height() as f64 - list.client_height() as f64).max(0.0);
    let next = (list.scroll_top() as f64 + dy).clamp(0.0, max);
    if (next - list.scroll_top() as f64).abs() < 0.5 {
        return;
    }
    let opts = web_sys::ScrollToOptions::new();
    opts.set_top(next);
    opts.set_behavior(if smooth {
        web_sys::ScrollBehavior::Smooth
    } else {
        web_sys::ScrollBehavior::Instant
    });
    list.scroll_to_with_scroll_to_options(&opts);
}

fn scroll_reader_line(dir: f64, smooth: bool) {
    let Some(list) = page_list() else { return };
    scroll_reader(dir * line_scroll_px(list.client_height() as f64), smooth);
}

fn scroll_reader_page(dir: f64, smooth: bool) {
    let Some(list) = page_list() else { return };
    scroll_reader(dir * page_scroll_px(list.client_height() as f64), smooth);
}

fn begin_line_hold(dir: f64) {
    HOLD_DIR.with(|d| d.set(dir));
    let now = js_sys::Date::now();
    HOLD_DOWN_AT.with(|t| t.set(now));
    HOLD_LAST.with(|t| t.set(now));
    // Tap = one smooth nudge. If the key is still down after HOLD_DELAY
    // the rAF loop takes over as a continuous glide.
    focus_page_list();
    scroll_reader_line(dir, true);
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
        scroll_reader(dir * HOLD_PX_PER_SEC * dt, false);
    }
    request_animation_frame(hold_tick);
}

/// Must be called once from the app root. `on_open` is the app's open-file
/// action (Cmd/Ctrl+O), injected so the viewer never depends on app chrome.
pub fn shortcuts(state: ViewerState, on_open: impl Fn() + 'static) {
    window_event_listener(leptos::ev::keydown, move |ev: leptos::ev::KeyboardEvent| {
        let key = ev.key();

        // Escape is a dismiss action, never text input, so it must work even
        // while a search input is focused — handle it before the form-target
        // guard. Closes the floating search overlay first, then the sidebar.
        if key == "Escape" {
            if state.search.visible.get() {
                // Closes the bar but leaves the muted highlights behind; the
                // next interaction with the document clears them.
                crate::effects::search_effects::dismiss_search(state);
            } else if state.sidebar.get() != SidebarMode::None {
                state.sidebar.set(SidebarMode::None);
            }
            return;
        }

        if is_form_target(&ev) {
            return;
        }

        let meta = ev.meta_key() || ev.ctrl_key();

        if meta {
            match key.to_lowercase().as_str() {
                // Cmd/Ctrl+O -> open dialog
                "o" => {
                    ev.prevent_default();
                    on_open();
                }
                // Cmd/Ctrl+0 -> fit width
                "0" => {
                    ev.prevent_default();
                    state.viewer.fit.set(FitMode::Width);
                }
                // Cmd/Ctrl+F -> open the floating search overlay
                "f" => {
                    ev.prevent_default();
                    // Resumes a just-dismissed search (query and all) instead
                    // of opening an empty bar.
                    crate::effects::search_effects::resume_search(state);
                }
                // Cmd/Ctrl+1 / 2 -> view mode
                "1" => {
                    ev.prevent_default();
                    state.viewer.mode.set(ViewMode::Single);
                }
                "2" => {
                    ev.prevent_default();
                    state.viewer.mode.set(ViewMode::Continuous);
                }
                _ => {}
            }
            return;
        }

        match key.as_str() {
            "+" | "=" => {
                ev.prevent_default();
                zoom_by(state, 1);
            }
            "-" | "_" => {
                ev.prevent_default();
                zoom_by(state, -1);
            }
            "ArrowLeft" => {
                ev.prevent_default();
                page_prev(state);
            }
            "ArrowRight" => {
                ev.prevent_default();
                page_next(state);
            }
            // Single-page: up/down turn the page. Continuous: WE scroll
            // `#page-list` ourselves — see `scroll_reader`. `repeat` is
            // ignored: the rAF hold loop is what keeps a held key gliding,
            // so the browser's discrete key-repeat cannot chunk the motion.
            "ArrowUp" => {
                if state.viewer.mode.get() == ViewMode::Single {
                    ev.prevent_default();
                    page_prev(state);
                } else if !is_chrome_scroll_target(&ev) {
                    ev.prevent_default();
                    if !ev.repeat() {
                        begin_line_hold(-1.0);
                    }
                }
            }
            "ArrowDown" => {
                if state.viewer.mode.get() == ViewMode::Single {
                    ev.prevent_default();
                    page_next(state);
                } else if !is_chrome_scroll_target(&ev) {
                    ev.prevent_default();
                    if !ev.repeat() {
                        begin_line_hold(1.0);
                    }
                }
            }
            "PageUp" => {
                if state.viewer.mode.get() == ViewMode::Continuous && !is_chrome_scroll_target(&ev)
                {
                    ev.prevent_default();
                    focus_page_list();
                    scroll_reader_page(-1.0, !ev.repeat());
                }
            }
            "PageDown" => {
                if state.viewer.mode.get() == ViewMode::Continuous && !is_chrome_scroll_target(&ev)
                {
                    ev.prevent_default();
                    focus_page_list();
                    scroll_reader_page(1.0, !ev.repeat());
                }
            }
            " " => {
                let on_button = ev
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::HtmlButtonElement>().ok())
                    .is_some();
                if !on_button
                    && state.viewer.mode.get() == ViewMode::Continuous
                    && !is_chrome_scroll_target(&ev)
                {
                    ev.prevent_default();
                    focus_page_list();
                    scroll_reader_page(if ev.shift_key() { -1.0 } else { 1.0 }, !ev.repeat());
                }
            }
            _ => {}
        }
    });

    // Release ends the rAF glide. Without this a held arrow would keep
    // scrolling after the key came up (or after the window lost focus).
    window_event_listener(
        leptos::ev::keyup,
        move |ev: leptos::ev::KeyboardEvent| match ev.key().as_str() {
            "ArrowUp" => end_line_hold(-1.0),
            "ArrowDown" => end_line_hold(1.0),
            _ => {}
        },
    );
    window_event_listener(leptos::ev::blur, move |_| stop_line_hold());
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
