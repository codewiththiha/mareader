//! workspace-ui: split shell, library explorer, active list and thumbnail proxy.
//! Depends on ui-common + library-core only. Must NOT depend on pdf-engine.
//! This is the host for panes; each pane is an isolated reader iframe.

pub use library_core;
pub use reader_core;
pub use ui_common;

/// Mount the workspace shell.
pub fn mount_workspace() {
    // Delegates to mareader::app::mount_workspace (built with library feature).
}

/// Workspace thumbnail proxy: implements ThumbnailSource via remote map
/// fed by the host's MessagePort, reusing the legacy virtualizer geometry.
pub mod thumbnail_proxy {
    use std::collections::HashMap;
    use leptos::prelude::*;
    // Bounded cache: visible ± 2 rows, evict far pages.
    pub const RETENTION_WINDOW: usize = 64;
    pub fn is_visible(page: u32, current: u32, window: usize) -> bool {
        let lo = current.saturating_sub(window as u32);
        let hi = current + window as u32;
        page >= lo && page <= hi
    }
    pub fn evict(cache: &mut HashMap<u32, String>, current: u32) {
        cache.retain(|k, _| is_visible(*k, current, RETENTION_WINDOW));
    }
}
