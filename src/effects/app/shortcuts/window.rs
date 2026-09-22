//! The Cmd/Ctrl combos: open, fit width, search, view mode.

use leptos::prelude::*;

use pdf_engine::types::DocStatus;
use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;
use crate::state::ReaderState;

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
                reader_app::effects::search::resume_search(state);
            } else {
                ui_kit::events::dispatch_event(ui_kit::events::FOCUS_LIBRARY_SEARCH_EVENT);
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
        _ => {}
    }
}
