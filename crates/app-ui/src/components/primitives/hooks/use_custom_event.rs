//! Window `CustomEvent` listeners: typed or raw, with the dispatch
//! element.

use std::rc::Rc;

use leptos::prelude::*;
use serde::de::DeserializeOwned;
use wasm_bindgen::JsValue;

/// Listen for a typed window CustomEvent, parsing `detail` into `T`.
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

/// Register one window CustomEvent listener, removed on cleanup.
fn listen(name: &'static str, handler: impl Fn(web_sys::CustomEvent) + 'static) {
    let handle = window_event_listener(leptos::ev::Custom::new(name), handler);
    on_cleanup(move || handle.remove());
}

/// Hand each raw `detail` to `on_event`, with the element it came from.
pub fn use_raw_event_from(
    name: &'static str,
    on_event: impl Fn(&JsValue, Option<web_sys::Element>) + 'static,
) {
    let on_event = Rc::new(on_event);
    listen(name, move |ev: web_sys::CustomEvent| {
        on_event(&ev.detail(), event_element(&ev))
    });
}
