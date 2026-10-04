//! Window `CustomEvent` plumbing: listen for an event — parsed back into a
//! type, or handed over raw — and hand the listener back with the element the
//! event was dispatched on. The dispatchers and the name table live in
//! `crate::events` (layer-neutral, so services can use them too); this module
//! keeps the reactive listener half.

use std::rc::Rc;

use leptos::prelude::*;
use serde::de::DeserializeOwned;
use wasm_bindgen::JsValue;

/// Listen for a typed window CustomEvent, parsing `detail` into `T` and
/// forwarding it with the element the event was dispatched ON (see
/// [`use_raw_event_from`]): `None` when it was dispatched on the window. The
/// listener is owned by the current reactive owner; malformed payloads are
/// dropped rather than panicking.
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

/// Hand each raw `detail` to `on_event` and let the caller make sense of it,
/// with the element the event was dispatched ON: the engine dispatches its
/// page events on the element they came from (the clicked link, the
/// selection's page host) and lets them bubble to the window, so a listener
/// that shares the window with other panes can tell whose event it is. `None`
/// when it was dispatched on the window itself (a clear, which is nobody's in
/// particular).
///
/// Raw rather than typed for those events because `null` is a third state
/// meaning "clear" rather than a malformed payload, and a `DeserializeOwned`
/// target cannot say the two apart. The listener is owned by the current
/// reactive owner — the half that matters, because a Leptos window listener
/// does NOT unregister when its handle is dropped.
pub fn use_raw_event_from(
    name: &'static str,
    on_event: impl Fn(&JsValue, Option<web_sys::Element>) + 'static,
) {
    let on_event = Rc::new(on_event);
    listen(name, move |ev: web_sys::CustomEvent| {
        on_event(&ev.detail(), event_element(&ev))
    });
}
