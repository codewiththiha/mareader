//! Global keyboard shortcuts. OWNED BY branch B (viewer/chrome).
//!
//! Must be called once from the app root (wired during integration).
//! The listener callback runs OUTSIDE the reactive owner, so everything it
//! touches is a Copy signal handle / AppState captured by value.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::components::molecules::toolbar::open_dialog;
use crate::core::layout::ViewMode;
use crate::core::math::{clamp_scale, nearest_zoom, FitMode};
use crate::core::state::{AppState, SidebarMode};

/// Applies a manual zoom step and exits fit mode.
fn zoom_by(state: AppState, dir: i32) {
    let cur = state.viewer.scale.get();
    let z = clamp_scale(nearest_zoom(cur, dir));
    state.viewer.fit.set(FitMode::None);
    state.viewer.scale.set(z);
    state.viewer.render_scale.set(z);
}

fn page_prev(state: AppState) {
    let p = state.viewer.page.get();
    if p > 1 {
        state.viewer.page.set(p - 1);
    }
}

fn page_next(state: AppState) {
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

/// Must be called once from the app root. Returns a handle so the caller can
/// remove it on cleanup if needed.
pub fn shortcuts(state: AppState) {
    window_event_listener(leptos::ev::keydown, move |ev: leptos::ev::KeyboardEvent| {
        if is_form_target(&ev) {
            return;
        }

        let meta = ev.meta_key() || ev.ctrl_key();
        let key = ev.key();

        if meta {
            match key.to_lowercase().as_str() {
                // Cmd/Ctrl+O -> open dialog
                "o" => {
                    ev.prevent_default();
                    open_dialog(state);
                }
                // Cmd/Ctrl+0 -> fit width
                "0" => {
                    ev.prevent_default();
                    state.viewer.fit.set(FitMode::Width);
                }
                // Cmd/Ctrl+F -> search sidebar + floating overlay
                "f" => {
                    ev.prevent_default();
                    state.sidebar.set(SidebarMode::Search);
                    state.search.visible.set(true);
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
            // In single-page mode up/down page; in continuous mode they scroll
            // naturally, so leave them alone.
            "ArrowUp" => {
                if state.viewer.mode.get() == ViewMode::Single {
                    ev.prevent_default();
                    page_prev(state);
                }
            }
            "ArrowDown" => {
                if state.viewer.mode.get() == ViewMode::Single {
                    ev.prevent_default();
                    page_next(state);
                }
            }
            // Escape closes the floating search overlay first, then the sidebar.
            "Escape" => {
                if state.search.visible.get() {
                    state.search.visible.set(false);
                } else if state.sidebar.get() != SidebarMode::None {
                    state.sidebar.set(SidebarMode::None);
                }
            }
            _ => {}
        }
    });
}
