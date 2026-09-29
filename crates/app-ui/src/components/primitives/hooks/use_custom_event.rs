//! Typed window `CustomEvent` plumbing: dispatch a serializable payload and
//! listen for it parsed back into the same type. The dispatcher and the name
//! table live in `crate::events` (layer-neutral, so services can use them
//! too); this module keeps the reactive listener half.

use std::rc::Rc;

use leptos::prelude::*;
use serde::de::DeserializeOwned;
use wasm_bindgen::JsValue;

pub use crate::events::dispatch_typed_event;

/// Listen for a typed window CustomEvent, parsing `detail` into `T` and
/// forwarding to `on_event`. The listener is owned by the current reactive
/// owner; malformed payloads are dropped rather than panicking.
pub fn use_typed_event<T: DeserializeOwned>(name: &'static str, on_event: impl Fn(T) + 'static) {
    let on_event = Rc::new(on_event);
    listen(name, move |ev: web_sys::CustomEvent| {
        if let Ok(v) = serde_wasm_bindgen::from_value::<T>(ev.detail()) {
            on_event(v);
        }
    });
}

/// [`use_typed_event`] plus the element the event was dispatched ON (see
/// [`use_raw_event_from`]): `None` when it was dispatched on the window.
pub fn use_typed_event_from<T: DeserializeOwned>(
    name: &'static str,
    on_event: impl Fn(T, Option<web_sys::Element>) + 'static,
) {
    let on_event = Rc::new(on_event);
    listen(name, move |ev: web_sys::CustomEvent| {
        if let Ok(v) = serde_wasm_bindgen::from_value::<T>(ev.detail()) {
            on_event(v, event_element(&ev));
        }
    });
}

/// The element an event was dispatched on, if it was one.
fn event_element(ev: &web_sys::CustomEvent) -> Option<web_sys::Element> {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
}

/// Register one window CustomEvent listener for the current owner and remove
/// it on cleanup — the half both hooks share, and the half that matters,
/// because a Leptos window listener does NOT unregister when its handle is
/// dropped.
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
pub fn use_raw_event(name: &'static str, on_detail: impl Fn(&JsValue) + 'static) {
    let on_detail = Rc::new(on_detail);
    listen(name, move |ev: web_sys::CustomEvent| {
        on_detail(&ev.detail())
    });
}

/// [`use_raw_event`] plus the element the event was dispatched ON: the
/// engine dispatches its page events on the element they came from (the
/// clicked link, the selection's page host) and lets them bubble to the
/// window, so a listener that shares the window with other panes can tell
/// whose event it is. `None` when it was dispatched on the window itself
/// (a clear, which is nobody's in particular).
pub fn use_raw_event_from(
    name: &'static str,
    on_event: impl Fn(&JsValue, Option<web_sys::Element>) + 'static,
) {
    let on_event = Rc::new(on_event);
    listen(name, move |ev: web_sys::CustomEvent| {
        on_event(&ev.detail(), event_element(&ev))
    });
}
