//! The Cmd/Ctrl combos: open, fit width, search, view mode, zoom.
//!
//! One arm per combo, and the two modifiers are deliberately not
//! distinguished: the caller routes Cmd and Ctrl through this one handler, so
//! macOS and Windows/Linux get the same keys by construction rather than by
//! two tables that can drift apart.

use leptos::prelude::*;

use crate::state::ReaderState;
use reader_core::document::DocStatus;
use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;

use super::zoom::zoom_by;

/// One Cmd/Ctrl combo. `on_open` is the app's open-file action, injected so
/// the shortcuts never depend on app chrome.
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
        // Cmd/Ctrl+F -> search what you are looking at: the document's floating
        // overlay while one is open, the library's title-bar filter while the
        // shelf is what is on screen.
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
        // Zoom, on every platform: the browser's own page-zoom combos are
        // claimed here and re-aimed at the document. `=` stands in for `+`
        // (the unshifted key of that cap on most layouts, and the one
        // reported when Shift is held on some), `_` for `-`, so the arm does
        // not depend on how the platform spells the shifted key.
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

// only the changed file was rewritten
