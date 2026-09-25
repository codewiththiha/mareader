//! The shell's bootstrap: the durable settings load, the shell state, the
//! bridge, and the shell's own effects. Runtimes create their own state
//! inside their sessions (§16) — nothing here provides a global app context.

use leptos::prelude::*;

use crate::state::ShellState;

pub(crate) fn create_shell_state() -> ShellState {
    ShellState {
        settings: RwSignal::new(storage::load_settings()),
        manager: std::sync::Arc::new(crate::app::manager::RuntimeManager::new()),
    }
}

/// The shell's own effects: theme, typography, motion — the paints that must
/// survive every runtime transition. Returns the appearance memo the effects
/// share.
pub(crate) fn install_shell_effects(state: ShellState) {
    let appearance: app_state::AppearanceSignal =
        Memo::new(move |_| state.settings.with(|s| s.appearance));
    crate::effects::app::typography::apply_typography(state.settings);
    crate::effects::app::motion::publish_motion(state.settings);
    crate::effects::app::theme::apply_theme(state, appearance);
}
