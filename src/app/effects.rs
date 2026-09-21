//! Effect ownership is explicit: persistent library versus disposable reader.
use crate::state::{AppState, AppearanceSignal};
use crate::state::reader::TypographySignal;

fn appearance(state: AppState, appearance: AppearanceSignal, typography: TypographySignal) {
    crate::effects::app::theme::apply_theme(state, appearance);
    crate::effects::app::typography::apply_typography(typography);
    crate::effects::app::motion::publish_motion(state);
}

#[cfg(feature = "library")]
pub fn install_library_effects(state: AppState, a: AppearanceSignal, t: TypographySignal) {
    appearance(state, a, t);
    shortcuts(state);
    crate::services::window::install_window_state_bridge(state);
    crate::effects::app::library::library_effects(state);
    // The OS handoff itself is host-owned, after library-ready.
}

pub fn install_reader_effects(state: AppState, a: AppearanceSignal, t: TypographySignal) {
    appearance(state, a, t);
    crate::effects::reader::blend_backdrop::paper_settings(state);
    crate::effects::reader::blend_epoch::install(state);
    shortcuts(state);
    crate::effects::reader::link_navigation::link_navigation(state);
    crate::effects::reader::page_selection::page_selection(state);
    crate::effects::reader::selection_tracking::selection_tracking(state);
    crate::services::ai::install_ai_chunk_bridge();
    crate::services::window::install_window_state_bridge(state);
}

fn shortcuts(state: AppState) {
    crate::effects::app::shortcuts::shortcuts(state.reader,
        move || crate::services::document::open_dialog(state), state.ui.sidebar);
}

#[cfg(feature = "library")]
pub fn install_workspace_effects(state: AppState, a: AppearanceSignal, t: TypographySignal) {
    appearance(state, a, t);
    shortcuts(state);
    crate::services::window::install_window_state_bridge(state);
}
