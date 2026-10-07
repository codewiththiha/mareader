//! The Cmd/Ctrl combos: open, fit width, search, mode, zoom.

use leptos::prelude::*;

use crate::state::ReaderState;
use reader_core::document::DocStatus;
use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;

use super::zoom::zoom_by;

/// One Cmd/Ctrl combo; `on_open` is injected.
pub(super) fn handle_modifier_shortcut<F: Fn() + 'static>(
    state: ReaderState,
    on_open: &F,
    ev: &leptos::ev::KeyboardEvent,
) {
    match ev.key().to_lowercase().as_str() {
        "o" => {
            ev.prevent_default();
            on_open();
        }
        "0" => {
            ev.prevent_default();
            state.viewer.fit.set(FitMode::Width);
        }
        // Cmd/Ctrl+F -> search what you are looking at.
        "f" => {
            ev.prevent_default();
            if state.document.status.get_untracked() == DocStatus::Ready {
                // Resumes a just-dismissed search (query and all) instead
                // of opening an empty bar.
                crate::effects::reader::search::resume_search(state);
            } else {
                app_ui::events::dispatch_event(app_ui::events::FOCUS_LIBRARY_SEARCH_EVENT);
            }
        }
        "1" => {
            ev.prevent_default();
            state.viewer.mode.set(ViewMode::Single);
        }
        "2" => {
            ev.prevent_default();
            state.viewer.mode.set(ViewMode::ScrollVertical);
        }
        // Zoom, re-aimed at the document on every platform.
        "+" | "=" => {
            ev.prevent_default();
            zoom_by(state, 1);
        }
        "-" | "_" => {
            ev.prevent_default();
            zoom_by(state, -1);
        }
        _ => {}
    }
}
