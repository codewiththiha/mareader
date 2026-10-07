//! The shell's bootstrap: the durable settings load, the state, and
//! the shell's own effects.

use leptos::prelude::*;

use crate::state::ShellState;

pub(crate) fn create_shell_state() -> ShellState {
    ShellState {
        settings: RwSignal::new(storage::load_settings()),
        manager: std::sync::Arc::new(crate::app::manager::RuntimeManager::new()),
    }
}

/// The shell's own effects: theme, typography and motion.
pub(crate) fn install_shell_effects(state: ShellState) {
    let appearance: app_state::AppearanceSignal =
        Memo::new(move |_| state.settings.with(|s| s.appearance));
    crate::effects::app::typography::apply_typography(state.settings);
    crate::effects::app::motion::publish_motion(state.settings);
    crate::effects::app::theme::apply_theme(state, appearance);
}

/// Back to Library is a real lifecycle transition.
pub(crate) fn install_history(state: ShellState) {
    let listener = window_event_listener(leptos::ev::popstate, move |event| {
        let path = web_sys::window()
            .and_then(|window| window.location().pathname().ok())
            .unwrap_or_default();
        if path == "/reader" {
            if let Ok(launch) = serde_wasm_bindgen::from_value::<
                runtime_contract::boundary::LaunchDocument,
            >(event.state())
                && !launch.path.is_empty()
            {
                let mut current =
                    crate::services::resolve_launch(&launch.path).unwrap_or_else(|| launch.clone());
                current.blend_override = launch.blend_override;
                state.manager.start_reader(&state, current);
                return;
            }
            crate::app::manager::RuntimeManager::boot(state.clone());
        } else {
            state.manager.start_library(&state);
        }
    });
    on_cleanup(move || listener.remove());
}
