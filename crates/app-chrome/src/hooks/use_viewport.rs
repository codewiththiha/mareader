//! Viewport size utilities: a pure snapshot read and a reactive resize-aware
//! signal. Shared by every floating surface (gloss card, selection pill,
//! popovers) so clamping math never re-implements the lookup.

use leptos::prelude::*;

/// The viewport size in CSS pixels. Read off `documentElement`'s bounding rect
/// rather than `window.innerWidth/Height` so it never depends on the "any"
/// return type of those getters and is unaffected by internal scrollers (this
/// app never lets the window itself scroll — `#page-list` does).
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

/// A reactive `(width, height)` viewport signal, kept fresh by a window
/// `resize` listener owned by the current reactive owner.
///
/// The resize listener is deliberately always-on rather than gated on
/// visibility: it is a single cheap listener and it makes "is my clamping
/// viewport stale" a non-question for any consumer (gloss, selection pill,
/// context menus).
pub fn use_viewport() -> RwSignal<(f64, f64)> {
    let size = RwSignal::new(viewport_size());
    super::use_window_event::use_window_event("resize", move |_| {
        size.set(viewport_size());
    });
    size
}
