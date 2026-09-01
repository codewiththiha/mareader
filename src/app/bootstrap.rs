//! Application bootstrap: the storage backend, the persisted state
//! (settings, library, covers), and the contexts the pages read (app
//! state, the viewer state slice, the appearance and texture signals).

use leptos::prelude::*;

use crate::state::AppState;
use crate::state::TextureSignal;
use crate::state::AppearanceSignal;
use crate::storage::{load_covers, load_library, load_settings};

/// App state seeded from the persisted settings/library/covers.
///
/// The three loads are synchronous localStorage reads plus a `serde_json`
/// parse, deliberately: they are on the order of a millisecond for a library
/// at its twenty-book cap, and every one of them has to be in hand before the
/// first paint — a theme that arrives a frame late is a visible flash of the
/// wrong palette, and a shelf that arrives late is a visible empty state.
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

/// Provide the app-level contexts: the app state (done by the caller), the
/// viewer slice of it, and the appearance/texture signals the page hosts need
/// (derived from settings; the viewer never touches settings itself).
///
/// Returns the appearance memo so the app root can hand it to the effects
/// that paint from it — one memo for the whole app, rather than one per
/// consumer each re-deriving the same slice.
pub(crate) fn provide_app_contexts(state: AppState) -> AppearanceSignal {
    // The look, narrowed out of the settings blob once. See
    // [`AppearanceSignal`] for why every DOM-writing consumer subscribes here
    // instead of to `settings`.
    let appearance: AppearanceSignal = Memo::new(move |_| state.settings.with(|s| s.appearance));
    // Narrowed again for the page hosts, which only care about the texture:
    // a tint nudge must not re-run their `texture-*` class.
    let texture: TextureSignal = Memo::new(move |_| appearance.get().texture);
    provide_context(state.reader);
    provide_context(appearance);
    provide_context(texture);
    // One overlay-lane registry for the whole app: menus and modals arbitrate
    // through it, and portaled surfaces resolve it like any other descendant
    // of the root.
    provide_context(crate::components::primitives::overlay::lanes::OverlayBoard::default());
    appearance
}
