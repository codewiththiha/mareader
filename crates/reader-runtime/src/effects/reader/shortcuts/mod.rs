//! Global keyboard shortcuts: the listeners, the guards, the routing.

mod auto_scroll;
mod keymap;
mod navigation;
mod window;
mod zoom;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::state::ReaderState;
use app_ui::components::shell::controller::ShellController;
use auto_scroll::handle_auto_scroll_shortcut;
use navigation::{end_hold_for, handle_navigation_shortcut, stop_hold};
use window::handle_modifier_shortcut;
use zoom::handle_zoom_shortcut;

/// True when the keydown target is a form control.
fn is_form_target(ev: &leptos::ev::KeyboardEvent) -> bool {
    ev.target().is_some_and(|target| {
        target.dyn_ref::<web_sys::HtmlInputElement>().is_some()
            || target.dyn_ref::<web_sys::HtmlSelectElement>().is_some()
    })
}

/// True when the key landed inside a chrome scroller.
fn is_chrome_scroll_target(ev: &leptos::ev::KeyboardEvent) -> bool {
    let Some(el) = ev
        .target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
    else {
        return false;
    };
    // One selector list, one ancestor walk: this runs on every keydown.
    el.closest(CHROME_SCROLL_SELECTOR).ok().flatten().is_some()
}

/// Chrome surfaces that own their own arrow keys.
const CHROME_SCROLL_SELECTOR: &str = "#thumb-scroll, aside, .menu-popover, [data-search-chrome]";

/// End any key hold still gliding: focus left this pane.
pub(crate) fn end_key_hold() {
    stop_hold();
}

/// Called once per pane, from its mount scope.
pub fn shortcuts(
    state: ReaderState,
    on_open: impl Fn() + 'static,
    shell: ShellController,
    is_active: impl Fn() -> bool + 'static,
) {
    // Derived once, here: per-keypress derivation would rebuild it.
    let sidebar_open = shell.is_sidebar_open();
    // Handles are parked and removed on cleanup; a dropped handle does
    // not unregister.
    let keydown =
        window_event_listener(leptos::ev::keydown, move |ev: leptos::ev::KeyboardEvent| {
            if !is_active() {
                return;
            }
            let key = ev.key();

            // Escape must work with an input focused, so it runs before the
            // guard.
            if key == "Escape" {
                if state.search.visible.get() {
                    // Closes the bar; the muted
                    // highlights go on the next
                    // interaction.
                    crate::effects::reader::search::dismiss_search(state);
                } else if sidebar_open.try_get_untracked() == Some(true)
                    && !app_chrome::floating::dismiss::escape_is_claimed()
                {
                    shell.close_sidebar();
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

            handle_auto_scroll_shortcut(state, &ev);
            handle_zoom_shortcut(state, &ev);
            handle_navigation_shortcut(state, &ev);
        });

    // Release ends the rAF glide, or a held arrow would keep going.
    let keyup = window_event_listener(leptos::ev::keyup, move |ev: leptos::ev::KeyboardEvent| {
        end_hold_for(&ev.key())
    });
    let blur = window_event_listener(leptos::ev::blur, move |_| stop_hold());

    let handles = StoredValue::new_local(vec![keydown, keyup, blur]);
    on_cleanup(move || {
        // A glide whose keyup will never arrive is stopped here.
        stop_hold();
        if let Some(handles) = handles.try_update_value(std::mem::take) {
            for handle in handles {
                handle.remove();
            }
        }
    });
}
