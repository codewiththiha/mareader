//! The reader's services: the document session and the AI bridge.

pub mod ai;
pub mod cefr;
pub mod document;

use web_sys::Event;

/// Tauri event taps: a no-op passthrough on the web build, forwarded to the
/// app-state shim.
pub fn tauri_listen(event: &str, handler: impl FnMut(Event) + 'static) {
    app_state::tauri_listen::tauri_listen(event, handler);
}
