//! Reloading the webview: the app's own restart, and the only reset for a
//! footprint that latched.
//!
//! The memory a long reading session leaves behind is the webview's, not a
//! list of objects the app can drop: WebKit returns freed arenas to the OS
//! only under pressure, and the wasm linear memory never shrinks at all, so
//! the number the OS reports sits at the session's high-water mark until the
//! page dies (`src/memory.rs`, Mareader.md "The memory model"). A reload is
//! what a force-quit does minus the quit — the one lever left, offered rather
//! than imposed.
//!
//! Which also means it owes exactly what a quit owes: the reading position the
//! progress effect is still debouncing. Flush it, then go.

use crate::context::ReaderContext;

/// Restart the app in place: flush the session's last write, then reload.
///
/// The reader lands back on the shelf, because that is where a cold boot
/// starts and nothing here pretends otherwise — the book keeps its resume
/// point, so reopening it lands where the reload found the reader.
pub fn reload_app(state: ReaderContext) {
    // The heap line before the reload is the one worth having: it is the
    // number the restart is about to give back, and after it the process is
    // new and every earlier reading is gone.
    app_state::memory::log_heap("reload");
    crate::services::document::flush_read_point(&state);
    app_chrome::window::api::reload_window();
}
