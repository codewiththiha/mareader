//! Is THIS runtime frame the one on screen?
//!
//! The Shell keeps up to three runtime frames alive at once — the active one,
//! a warm one booted behind it, and a retiring one being disposed — and marks
//! each `<iframe>` with `data-mareader-slot`. Every frame mounts its own
//! chrome, so anything that drives a WINDOW-level resource (the native macOS
//! traffic lights are one pair per window, not per document) must only be
//! driven by the active frame. Otherwise a hidden warm shelf with a pinned
//! bar re-lights the buttons over a reader whose bar is hidden, or a hidden
//! reader's hover-out hides them under a shelf whose bar is showing.
//!
//! Same-origin frames can read `window.frameElement`; the observer watches
//! that element's slot attribute so a promotion flips the signal in the
//! frame that was just revealed. A top-level document (standalone artifact,
//! the unified dev build) has no frame element and is always active.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

/// The attribute the Shell's frame driver writes (`src/app/frame.rs`).
pub const SLOT_ATTR: &str = "data-mareader-slot";

fn frame_element() -> Option<web_sys::Element> {
    web_sys::window()?.frame_element().ok().flatten()
}

fn slot_is_active(el: &web_sys::Element) -> bool {
    // No attribute yet means a frame the Shell has not classified — the
    // pre-warm-slot boot path, which only ever creates visible frames.
    el.get_attribute(SLOT_ATTR)
        .is_none_or(|slot| slot == "active")
}

/// Whether this document is the frame on screen right now. One-shot read.
pub fn frame_is_active() -> bool {
    frame_element().is_none_or(|el| slot_is_active(&el))
}

/// A reactive [`frame_is_active`]: flips when the Shell promotes or retires
/// this frame. Owned by the calling component; the observer disconnects on
/// cleanup.
pub fn use_frame_active() -> Signal<bool> {
    let Some(el) = frame_element() else {
        return Signal::derive(|| true);
    };
    let active = RwSignal::new(slot_is_active(&el));
    let watched = el.clone();
    let callback = Closure::<dyn FnMut(js_sys::Array, web_sys::MutationObserver)>::new(
        move |_records: js_sys::Array, _observer: web_sys::MutationObserver| {
            let now = slot_is_active(&watched);
            if active.try_get_untracked() != Some(now) {
                let _ = active.try_set(now);
            }
        },
    );
    if let Ok(observer) = web_sys::MutationObserver::new(callback.as_ref().unchecked_ref()) {
        let init = web_sys::MutationObserverInit::new();
        init.set_attributes(true);
        init.set_attribute_filter(&js_sys::Array::of1(&SLOT_ATTR.into()));
        let _ = observer.observe_with_options(&el, &init);
        let held = StoredValue::new_local(Some((observer, callback)));
        on_cleanup(move || {
            if let Some((observer, _callback)) = held.try_update_value(|v| v.take()).flatten() {
                observer.disconnect();
            }
        });
    }
    active.into()
}
