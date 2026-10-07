//! The Shift+A auto-scroll toggle.

use leptos::prelude::*;

use crate::state::ReaderState;

/// Shift+A arms the drift, only where a drift can run.
pub(super) fn handle_auto_scroll_shortcut(state: ReaderState, ev: &leptos::ev::KeyboardEvent) {
    if ev.shift_key() && ev.key().to_lowercase() == "a" {
        ev.prevent_default();
        if state.viewer.mode.get().can_scroll() {
            let on = state.viewer.auto_scroll.get();
            state.viewer.auto_scroll.set(!on);
        }
    }
}
