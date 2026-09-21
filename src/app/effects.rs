//! The app-root effects, installed once and in the one order that works.
//!
//! One entry point gives the ordering contract a home: two of the steps are
//! order-dependent in ways that fail silently.
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
//! 5. The app-lifetime Tauri listeners, in any order between them:
//!    `install_ai_chunk_bridge` (AI chunks) and `install_window_state_bridge`
//!    (the frameless maximize flag). Then `library_effects`
//!    (`crate::effects::app::library`): the library's progress sink, the
//!    startup measurement pass, and a rescan of the watched folders once it
//!    has — before the OS handoff below, so a double-clicked book never lands
//!    in the middle of that first pass.
//! 6. `init_open_file_handling` — LAST, and the step the ordering is really
//!    for: it can open a document IMMEDIATELY (a double-clicked file hands the
//!    backend a path before the webview finishes mounting), so every step
//!    above must have run by then.
//! 7. `log_heap("boot")` — the memory probe's idle row
//!    (`docs/memory-baseline.md`), exactly once per webview, and before any
//!    document can log `open`: opening is IPC work, this line logs in the
//!    same synchronous breath that finishes the install.
//!
//! INSTALLED ONCE. Each arm registers a window listener, a Tauri subscription,
//! or both, and none unsubscribe — they live as long as the app. That is wrong
//! for a second mount (hot reload, hydration retry), where listeners would
//! stack and every keystroke be handled twice; the guard below makes the
//! second install a no-op.

use std::sync::atomic::{AtomicBool, Ordering};

use reader_app::state::TypographySignal;
use crate::effects::app::motion::publish_motion;
use crate::effects::app::theme::apply_theme;
use crate::effects::app::typography::apply_typography;
use reader_app::effects::blend_backdrop::paper_settings;
use reader_app::effects::link_navigation::link_navigation;
use reader_app::effects::page_selection::page_selection;
use reader_app::effects::selection_tracking::selection_tracking;
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

    apply_theme(state, appearance);
    apply_typography(typography);
    paper_settings(state.settings);
    publish_motion(state);
    shortcuts(state);
    link_navigation(state.reader);
    page_selection(state.reader);
    selection_tracking(state.reader);
    crate::services::ai::install_ai_chunk_bridge();
    crate::services::window::install_window_state_bridge(state);
    crate::effects::app::library::library_effects(state);
    crate::services::document::init_open_file_handling(state);

    // The memory probe's idle row. The INSTALLED guard above is what makes
    // the "exactly once" certain, and sitting after the whole install keeps
    // it ahead of every `open` the log will ever chart: opening a document
    // is IPC work, this line logs in the same synchronous breath that
    // finishes the install. The baseline tables in `docs/memory-baseline.md`
    // read their deltas against this row.
    crate::memory::log_heap("boot");
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
