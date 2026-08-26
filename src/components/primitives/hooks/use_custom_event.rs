//! Typed window `CustomEvent` plumbing: dispatch a serializable payload and
//! listen for it parsed back into the same type. This is the app's one
//! cross-cutting message mechanism (gloss open / context, chunk events,
//! reveal-active, link jumps) so every variant stops re-implementing the
//! `serde_wasm_bindgen` round-trip.

use std::rc::Rc;

use leptos::prelude::*;
use serde::{de::DeserializeOwned, Serialize};

/// Dispatch a typed CustomEvent on `window` with `payload` as its detail.
pub fn dispatch_typed_event<T: Serialize>(name: &str, payload: &T) {
    let Some(win) = web_sys::window() else {
        return;
    };
    let Ok(detail) = serde_wasm_bindgen::to_value(payload) else {
        return;
    };
    let init = web_sys::CustomEventInit::new();
    init.set_detail(&detail);
    if let Ok(ev) = web_sys::CustomEvent::new_with_event_init_dict(name, &init) {
        let _ = win.dispatch_event(&ev);
    }
}

/// Dispatch a payload-less CustomEvent on `window` (one-shot gestures like
/// "reveal the active row").
/// Listen for a typed window CustomEvent, parsing `detail` into `T` and
/// forwarding to `on_event`. The listener is owned by the current reactive
/// owner; malformed payloads are dropped rather than panicking.
pub fn use_typed_event<T: DeserializeOwned>(name: &'static str, on_event: impl Fn(T) + 'static) {
    let on_event = Rc::new(on_event);
    let handle = window_event_listener(
        leptos::ev::Custom::new(name),
        move |ev: web_sys::CustomEvent| {
            if let Ok(v) = serde_wasm_bindgen::from_value::<T>(ev.detail()) {
                on_event(v);
            }
        },
    );
    on_cleanup(move || handle.remove());
}
