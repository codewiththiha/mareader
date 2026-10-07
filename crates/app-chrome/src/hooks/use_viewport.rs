//! Viewport size utilities: a snapshot read and a reactive signal.

use leptos::prelude::*;

/// The viewport size in CSS pixels, off `documentElement`'s rect.
pub fn viewport_size() -> (f64, f64) {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
        .map(|el| {
            let r = el.get_bounding_client_rect();
            (r.width(), r.height())
        })
        .unwrap_or((0.0, 0.0))
}

/// A reactive `(width, height)` viewport signal, resize-aware.
pub fn use_viewport() -> RwSignal<(f64, f64)> {
    let size = RwSignal::new(viewport_size());
    super::use_window_event::use_window_event("resize", move |_| {
        size.set(viewport_size());
    });
    size
}
