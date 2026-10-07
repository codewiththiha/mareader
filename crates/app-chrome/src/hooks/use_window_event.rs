//! Owner-scoped window event listeners, bubble and capture.

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use leptos::prelude::*;

/// Register a bubble-phase `window` listener for the current owner.
pub fn use_window_event(event: &'static str, handler: impl Fn(web_sys::Event) + 'static) {
    add_owned(event, false, handler);
}

/// Add a capture-phase `window` listener for the current owner.
pub fn add_window_capture_listener(event: &str, handler: impl FnMut(web_sys::Event) + 'static) {
    add_owned(event, true, handler);
}

/// The registration both flavours share.
fn add_owned(event: &str, capture: bool, handler: impl FnMut(web_sys::Event) + 'static) {
    let cb: Closure<dyn FnMut(web_sys::Event)> =
        Closure::wrap(Box::new(handler) as Box<dyn FnMut(web_sys::Event)>);
    let win = web_sys::window().expect("window");
    let f: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
    let _ = win.add_event_listener_with_callback_and_bool(event, &f, capture);

    let cb_store = StoredValue::new_local(Some(cb));
    let f_store = StoredValue::new_local(Some(f));
    let event_owned = event.to_string();
    on_cleanup(move || {
        if let Some(f) = f_store.try_get_value().flatten()
            && let Some(win) = web_sys::window()
        {
            let _ = win.remove_event_listener_with_callback_and_bool(&event_owned, &f, capture);
        }
        let _ = cb_store.try_set_value(None);
    });
}
