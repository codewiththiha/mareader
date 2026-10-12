//! The reader's services: the document session and the AI bridge.

pub mod ai;
pub mod cefr;
pub mod dict;
pub mod document;

use std::sync::atomic::{AtomicUsize, Ordering};

use web_sys::Event;

/// Backend taps installed in this realm; the diagnostics gauge reads it.
static BRIDGE_LISTENERS: AtomicUsize = AtomicUsize::new(0);

/// Tauri event taps: a no-op passthrough on the web build, forwarded to the
/// app-state shim.
pub fn tauri_listen(event: &str, handler: impl FnMut(Event) + 'static) {
    app_state::tauri_listen::tauri_listen(event, handler);
}

/// Note one tap installed here; the once-guard has already run.
pub(crate) fn note_bridge_listener() {
    BRIDGE_LISTENERS.fetch_add(1, Ordering::Relaxed);
}

/// Taps this realm installed, for the diagnostics gauge.
pub fn bridge_listeners() -> usize {
    BRIDGE_LISTENERS.load(Ordering::Relaxed)
}
