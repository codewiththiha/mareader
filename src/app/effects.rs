//! The app-root effects, installed once and in the one order that works.
//!
//! These used to be a bare run of calls in `App` with the ordering contract in
//! comments beside them — nothing enforced it, and two of the steps are
//! order-dependent in ways that fail silently. One entry point gives the
//! contract a home.
//!
//! THE ORDER, and why each step is where it is:
//!
//! 1. `apply_theme` + `apply_typography` — both page kinds paint from the
//!    custom properties these write, so they must be on `<html>` before the
//!    first frame; late, the reader flashes the untinted palette (or a text
//!    document the default type).
//! 2. `paper_settings` — the paper session's blend and detection settings must
//!    land before the FIRST document opens: the open flow asks the engine's
//!    per-document colour cache under the reader's real settings, earlier than
//!    any reader mounts. Asked under defaults, the first book's backdrop is
//!    quietly the wrong colour.
//! 3. `publish_motion` — the reduced-motion projection, needed by the reader's
//!    pipeline and by CSS the app does not model.
//! 4. The input and selection arms, in any order among themselves.
//! 5. The two app-lifetime Tauri listeners, in any order between them:
//!    `install_ai_chunk_bridge` (AI chunks) and `install_window_state_bridge`
//!    (the frameless maximize flag).
//! 6. `init_open_file_handling` — LAST, and the step the ordering is really
//!    for: it can open a document IMMEDIATELY (a double-clicked file hands the
//!    backend a path before the webview finishes mounting), so every step
//!    above must have run by then.
//!
//! INSTALLED ONCE. Each arm registers a window listener, a Tauri subscription,
//! or both, and none unsubscribe — they live as long as the app. That is wrong
//! for a second mount (hot reload, hydration retry), where listeners would
//! stack and every keystroke be handled twice; the guard below makes the
//! second install a no-op.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::state::reader::TypographySignal;
use crate::effects::app::motion::publish_motion;
use crate::effects::app::theme::apply_theme;
use crate::effects::app::typography::apply_typography;
use crate::effects::reader::blend_backdrop::paper_settings;
use crate::effects::reader::link_navigation::link_navigation;
use crate::effects::reader::page_selection::page_selection;
use crate::effects::reader::selection_tracking::selection_tracking;
use crate::state::{AppState, AppearanceSignal};

/// Whether the app-root effects are already installed. Relaxed ordering: the
/// webview is single-threaded, so this only has to be a flag, never a fence.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Install every app-lifetime effect, in the order documented above. Safe to
/// call more than once — later calls do nothing.
pub(crate) fn install_app_effects(
    state: AppState,
    appearance: AppearanceSignal,
    typography: TypographySignal,
) {
    if INSTALLED.swap(true, Ordering::Relaxed) {
        return;
    }

    // 1. The look, before the first paint: the palette, and the reflowable
    //    formats' typography variables.
    apply_theme(state, appearance);
    apply_typography(state, typography);
    // 2. The paper session, before the first open (see the module doc).
    paper_settings(state);
    // 3. Motion preferences, for the reader's pipeline and for the CSS.
    publish_motion(state);
    // 4. Input and selection.
    shortcuts(state);
    link_navigation(state);
    page_selection(state);
    selection_tracking(state);
    // 5. One Tauri AI-chunk listener for the app's life; re-broadcasts as a
    //    window event so the gloss popover never stacks or drops handlers
    //    across document switches.
    crate::services::ai::install_ai_chunk_bridge();
    // 5b. The frameless maximize flag: one resize subscription publishing
    //     into UiState, so the caption cluster never owns a listener.
    crate::services::window::install_window_state_bridge(state);
    // 6. OS file opening: double-click / "Open with" / default-app launch.
    //    Last, because it can open a document on the spot.
    crate::services::document::init_open_file_handling(state);
}

/// Global keyboard shortcuts; the open-file action is injected from the app so
/// the viewer crate never depends on app chrome.
fn shortcuts(state: AppState) {
    crate::effects::app::shortcuts::shortcuts(
        state.reader,
        move || crate::services::document::open_dialog(state),
        state.ui.sidebar,
    );
}
