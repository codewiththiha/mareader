//! Typed window `CustomEvent` plumbing: dispatch a serializable payload and
//! listen for it parsed back into the same type. The dispatcher and the name
//! table live in `crate::events` (layer-neutral, so services can use them
//! too); this module keeps the reactive listener half.

use std::rc::Rc;

use leptos::prelude::*;
use serde::de::DeserializeOwned;

pub use crate::events::dispatch_typed_event;

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
