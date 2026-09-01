//! Global keyboard shortcuts: the keydown/keyup/blur listeners, the
//! target guards, and the routing to the per-intent handlers
//! (`window` combos, `zoom` steps, `navigation` + the scroll-hold engine).
//!
//! Must be called once from the app root. The listener callback runs
//! OUTSIDE the reactive owner, so everything it touches is a Copy signal
//! handle / ReaderState captured by value.

mod keymap;
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
    // One selector list, one ancestor walk. This runs on EVERY keydown, and
    // asking `closest` four times walked the tree to the root four times over
    // before concluding that a key pressed over the document is the document's.
    el.closest(CHROME_SCROLL_SELECTOR)
        .ok()
        .flatten()
        .is_some()
}

/// Chrome surfaces that own their own arrow keys.
const CHROME_SCROLL_SELECTOR: &str = "#thumb-scroll, aside, .menu-popover, [data-search-chrome]";

/// Must be called once from the app root. `on_open` is the app's open-file
/// action (Cmd/Ctrl+O), injected so the viewer never depends on app chrome.
pub fn shortcuts(
    state: ReaderState,
    on_open: impl Fn() + 'static,
    // Sidebar mode is read/written for the panel toggles (app chrome
    // state passed in explicitly).
    sidebar: RwSignal<SidebarMode>,
) {
    // Handles are parked and removed on cleanup. In practice the app root
    // installs these once for the process lifetime, but a dropped handle does
    // NOT unregister the listener — so an owner that ever went away would
    // leave a keydown handler behind, still holding its state signals and
    // still driving the scroll-hold engine.
    let keydown = window_event_listener(leptos::ev::keydown, move |ev: leptos::ev::KeyboardEvent| {
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

        crate::effects::reader::auto_scroll::handle_auto_scroll_shortcut(state, &ev);
        handle_zoom_shortcut(state, &ev);
        handle_navigation_shortcut(state, &ev);
    });

    // Release ends the rAF glide. Without this a held arrow would keep
    // scrolling after the key came up (or after the window lost focus).
    let keyup = window_event_listener(leptos::ev::keyup, move |ev: leptos::ev::KeyboardEvent| {
        end_hold_for(&ev.key())
    });
    let blur = window_event_listener(leptos::ev::blur, move |_| stop_hold());

    let handles = StoredValue::new_local(vec![keydown, keyup, blur]);
    on_cleanup(move || {
        // Any glide still running belongs to a keyup that will now never
        // arrive, so it is stopped here rather than left on the rAF loop.
        stop_hold();
        if let Some(handles) = handles.try_update_value(std::mem::take) {
            for handle in handles {
                handle.remove();
            }
        }
    });
}
