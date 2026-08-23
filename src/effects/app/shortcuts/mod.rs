//! Global keyboard shortcuts: the keydown/keyup/blur listeners, the
//! target guards, and the routing to the per-intent handlers
//! (`window` combos, `zoom` steps, `navigation` + the scroll-hold engine).
//!
//! Must be called once from the app root. The listener callback runs
//! OUTSIDE the reactive owner, so everything it touches is a Copy signal
//! handle / ReaderState captured by value.

mod navigation;
mod window;
mod zoom;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::state::{ReaderState, SidebarMode};
use navigation::{end_hold_for, handle_navigation_shortcut, stop_hold};
use window::handle_modifier_shortcut;
use zoom::handle_zoom_shortcut;

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

/// Must be called once from the app root. `on_open` is the app's open-file
/// action (Cmd/Ctrl+O), injected so the viewer never depends on app chrome.
pub fn shortcuts(
    state: ReaderState,
    on_open: impl Fn() + 'static,
    // Sidebar mode is read/written for the panel toggles (app chrome
    // state passed in explicitly).
    sidebar: RwSignal<SidebarMode>,
) {
    window_event_listener(leptos::ev::keydown, move |ev: leptos::ev::KeyboardEvent| {
        let key = ev.key();

        // Escape is a dismiss action, never text input, so it must work even
        // while a search input is focused — handle it before the form-target
        // guard. Closes the floating search overlay first, then the sidebar.
        if key == "Escape" {
            if state.search.visible.get() {
                // Closes the bar but leaves the muted highlights behind; the
                // next interaction with the document clears them.
                crate::effects::reader::search::dismiss_search(state);
            } else if sidebar.get() != SidebarMode::None {
                sidebar.set(SidebarMode::None);
            }
            return;
        }

        if is_form_target(&ev) {
            return;
        }

        if ev.meta_key() || ev.ctrl_key() {
            handle_modifier_shortcut(state, &on_open, &ev);
            return;
        }

        handle_zoom_shortcut(state, &ev);
        handle_navigation_shortcut(state, &ev);
    });

    // Release ends the rAF glide. Without this a held arrow would keep
    // scrolling after the key came up (or after the window lost focus).
    window_event_listener(leptos::ev::keyup, move |ev: leptos::ev::KeyboardEvent| {
        end_hold_for(&ev.key())
    });
    window_event_listener(leptos::ev::blur, move |_| stop_hold());
}
