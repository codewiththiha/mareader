//! The Cmd/Ctrl combos: open, fit width, search, view mode.

use leptos::prelude::*;

#[cfg(all(format_runtime, target_arch = "wasm32"))]
use reader_core::DocStatus;
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
            #[cfg(all(format_runtime, target_arch = "wasm32"))]
            if state.document.status.get_untracked() == DocStatus::Ready {
                crate::effects::reader::search::resume_search(state);
            } else {
                crate::events::dispatch_event(crate::events::FOCUS_LIBRARY_SEARCH_EVENT);
            }
            #[cfg(not(format_runtime))]
            {
                let _ = state;
                crate::events::dispatch_event(crate::events::FOCUS_LIBRARY_SEARCH_EVENT);
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
