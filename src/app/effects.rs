//! The app-root effects, installed once, per session.
//!
//! The shelf and the reader are different pages (`src/boot.rs`). Each installs
//! only what that page can use. A second mount in the same page (hot reload)
//! is still a no-op: the listeners do not unsubscribe, and stacking them would
//! handle every keystroke twice.
//!
//! Shelf order: paint, input, the Tauri bridges, the flush hook, then the
//! library's own startup (progress sink, measurement, rescan), and the OS-file
//! handoff last so a double-clicked book does not land in the middle of that
//! pass. The shelf does not publish paper settings and does not install reader
//! effects. It never opens a document in this heap.
//!
//! Reader order: paint, then `paper_settings` — the blend must be published
//! before the handoff open asks the engine for a colour — then the reader
//! input arms, the bridges, the flush hook, and the OS-file handoff last. No
//! shelf rescan and no cover backfill: both would fight the book this page is
//! about to open, and the next shelf boot runs them.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::effects::app::motion::publish_motion;
use crate::effects::app::theme::apply_theme;
use crate::effects::app::typography::apply_typography;
use crate::effects::reader::blend_backdrop::paper_settings;
use crate::effects::reader::link_navigation::link_navigation;
use crate::effects::reader::page_selection::page_selection;
use crate::effects::reader::selection_tracking::selection_tracking;
use crate::state::reader::TypographySignal;
use crate::state::{AppState, AppearanceSignal};

/// Whether this page's effects are already installed. Relaxed ordering: the
/// webview is single-threaded, so this only has to be a flag, never a fence.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Shelf page. See the module note for the order.
pub(crate) fn install_library_session(
    state: AppState,
    appearance: AppearanceSignal,
    typography: TypographySignal,
) {
    if INSTALLED.swap(true, Ordering::Relaxed) {
        return;
    }
    apply_theme(state, appearance);
    apply_typography(typography);
    publish_motion(state);
    shortcuts(state);
    crate::services::ai::install_ai_chunk_bridge();
    crate::services::window::install_window_state_bridge(state);
    crate::boot::install_flush(state);
    crate::effects::app::library::library_effects(state);
    crate::services::document::init_open_file_handling(state);
}

/// Reader page. `paper_settings` is synchronous and lands before the OS-file
/// pull and before the component applies the handoff.
pub(crate) fn install_reader_session(
    state: AppState,
    appearance: AppearanceSignal,
    typography: TypographySignal,
) {
    if INSTALLED.swap(true, Ordering::Relaxed) {
        return;
    }
    apply_theme(state, appearance);
    apply_typography(typography);
    paper_settings(state);
    publish_motion(state);
    shortcuts(state);
    link_navigation(state);
    page_selection(state);
    selection_tracking(state);
    crate::services::ai::install_ai_chunk_bridge();
    crate::services::window::install_window_state_bridge(state);
    crate::boot::install_flush(state);
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
