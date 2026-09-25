//! The reader's services: the document session (open/close/flush/seed) and
//! the AI bridge. Library-side operations (import, arrange, the shelf) are
//! the library runtime's; this tree reaches them only through the boundary.

pub mod ai;
pub mod document;
pub mod reload;

use web_sys::Event;

/// Tauri event taps: a no-op passthrough on the web build, forwarded to the
/// app-state shim.
pub fn tauri_listen(event: &str, handler: impl FnMut(Event) + 'static) {
    app_state::tauri_listen::tauri_listen(event, handler);
}
