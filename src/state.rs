//! The Shell's own state: what survives every runtime transition.

use leptos::prelude::*;
use reader_core::settings::Settings;

use crate::app::manager::RuntimeManager;

/// The settings value the shell's persistence writes.
#[derive(Clone)]
pub struct ShellState {
    pub settings: RwSignal<Settings>,
    /// An `Arc` shell: every clone shares the ONE manager.
    pub manager: std::sync::Arc<RuntimeManager>,
}

/// What the diagnostics probe reports about the shell itself.
#[derive(Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ActiveRuntime {
    Library,
    Reader,
}
