//! The Shell's own state: what survives every runtime transition. Deliberately
//! tiny (§2) — routing, the manager's slot, the durable settings, and the
//! diagnostics caches. No document proxies, no rasters, no reader state.

use leptos::prelude::*;
use reader_core::settings::Settings;

use crate::app::manager::RuntimeManager;

/// The settings value the shell's own persistence writes; the shell never
/// EDITS settings (menus live in runtimes) — it loads, saves, and seeds.
#[derive(Clone)]
pub struct ShellState {
    pub settings: RwSignal<Settings>,
    /// An `Arc` shell: every clone shares the ONE manager (one navigation
    /// authority, §10) — a cloned handle is the same authority, never a copy.
    pub manager: std::sync::Arc<RuntimeManager>,
}

/// What the diagnostics probe reports about the shell itself.
#[derive(Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ActiveRuntime {
    Library,
    Reader,
}
