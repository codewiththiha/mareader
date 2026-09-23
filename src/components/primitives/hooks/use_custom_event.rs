//! Typed window `CustomEvent` plumbing: dispatch a serializable payload and
//! listen for it parsed back into the same type. The dispatcher and the name
//! table live in `crate::events` (layer-neutral, so services can use them
//! too); this module keeps the reactive listener half.

#[cfg(all(format_runtime, target_arch = "wasm32"))]
use std::rc::Rc;

#[cfg(all(format_runtime, target_arch = "wasm32"))]
use leptos::prelude::*;
#[cfg(all(format_runtime, target_arch = "wasm32"))]
use serde::de::DeserializeOwned;
#[cfg(all(format_runtime, target_arch = "wasm32"))]
use wasm_bindgen::JsValue;

#[cfg(all(format_runtime, target_arch = "wasm32"))]
pub use crate::events::dispatch_typed_event;

/// Listen for a typed window CustomEvent, parsing `detail` into `T` and
/// forwarding to `on_event`. The listener is owned by the current reactive
/// owner; malformed payloads are dropped rather than panicking.
#[cfg(all(format_runtime, target_arch = "wasm32"))]
pub fn use_typed_event<T: DeserializeOwned>(name: &'static str, on_event: impl Fn(T) + 'static) {
    let on_event = Rc::new(on_event);
    listen(name, move |ev: web_sys::CustomEvent| {
        if let Ok(v) = serde_wasm_bindgen::from_value::<T>(ev.detail()) {
            on_event(v);
        }
    });
}

/// Register one window CustomEvent listener for the current owner and remove
/// it on cleanup — the half both hooks share, and the half that matters,
/// because a Leptos window listener does NOT unregister when its handle is
/// dropped.
#[cfg(all(format_runtime, target_arch = "wasm32"))]
fn listen(name: &'static str, handler: impl Fn(web_sys::CustomEvent) + 'static) {
    let handle = window_event_listener(leptos::ev::Custom::new(name), handler);
    on_cleanup(move || handle.remove());
}

/// The untyped sibling of [`use_typed_event`]: hand each raw `detail` to
/// `on_detail` and let the caller make sense of it.
///
/// For the engine's selection and navigation events, where `null` is a third
/// state meaning "clear" rather than a malformed payload. A `DeserializeOwned`
/// target cannot say the two apart, so those keep their own parse and share
/// only this half — which is the half that matters, because a Leptos window
/// listener does NOT unregister when its handle is dropped. Every effect that
/// installed one used to carry a comment saying so; this is the one place that
/// now has to.
#[cfg(all(format_runtime, target_arch = "wasm32"))]
pub fn use_raw_event(name: &'static str, on_detail: impl Fn(&JsValue) + 'static) {
    let on_detail = Rc::new(on_detail);
    listen(name, move |ev: web_sys::CustomEvent| on_detail(&ev.detail()));
}
