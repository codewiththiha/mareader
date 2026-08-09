//! Global keyboard shortcuts. OWNED BY branch B (viewer/chrome).

use crate::core::state::AppState;

/// Must be called once from the app root. Returns a handle so the caller can
/// remove it on cleanup if needed.
pub fn shortcuts(_state: AppState) {
    // TODO(branch B): Cmd/Ctrl+O open, Cmd/Ctrl+0 reset zoom, +/- zoom,
    // arrows page nav (respecting view mode), Cmd/Ctrl+F search, toggle sidebar.
}
