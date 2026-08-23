//! The Cmd/Ctrl combos: open, fit width, search, view mode.

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use pdf_core::math::FitMode;
use crate::state::ReaderState;

/// One Cmd/Ctrl combo. `on_open` is the app's open-file action, injected so
/// the shortcuts never depend on app chrome.
pub(super) fn handle_modifier_shortcut<F: Fn() + 'static>(
    state: ReaderState,
    on_open: &F,
    ev: &leptos::ev::KeyboardEvent,
) {
    match ev.key().to_lowercase().as_str() {
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
            crate::effects::reader::search::resume_search(state);
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
}
