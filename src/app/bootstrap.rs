//! Application bootstrap: the storage backend, the persisted state
//! (settings, library, covers), and the contexts the pages read (app
//! state, the viewer state slice, the page texture signal).

use leptos::prelude::*;

use crate::state::TextureSignal;
use crate::state::AppState;
use crate::storage::{load_covers, load_library, load_settings};

/// App state seeded from the persisted settings/library/covers.
pub(crate) fn create_app_state() -> AppState {
    AppState {
        settings: RwSignal::new(load_settings()),
        library: crate::state::library::LibraryState {
            books: RwSignal::new(load_library()),
            covers: RwSignal::new(load_covers()),
        },
        ..AppState::default()
    }
}

/// Provide the app-level contexts: the app state (done by the caller),
/// the viewer slice of it, and the texture signal the page hosts need
/// (derived from settings; the viewer never touches settings itself).
pub(crate) fn provide_app_contexts(state: AppState) {
    let texture: TextureSignal = Memo::new(move |_| {
        state.settings.with(|s| s.appearance.texture)
    });
    provide_context(state.reader);
    provide_context(texture);
    // One overlay-lane registry for the whole app: menus and modals arbitrate
    // through it, and portaled surfaces resolve it like any other descendant
    // of the root.
    provide_context(crate::components::primitives::overlay::lanes::OverlayBoard::default());
}
