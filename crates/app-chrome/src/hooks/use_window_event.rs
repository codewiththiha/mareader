//! Owner-scoped window event listeners. Two flavours:
//!
//! * [`use_window_event`] — bubble phase, typed exactly like leptos' own
//!   `window_event_listener` but with the closure parked in a `StoredValue`
//!   so a re-run of the effect cannot free a live wasm-shim closure mid-queue.
//! * [`add_window_capture_listener`] — capture phase, for events that do not
//!   bubble (`scroll`) but still hit `window` in the capture phase.
//!
//! Both are one registration — [`add_owned`] below — with the capture flag set
//! differently, because the part that is easy to get wrong (parking the closure
//! for the owner's lifetime, removing it before the park is released) is the
//! same part in both.
//!
//! Both must be called from inside a reactive scope (an `Effect`); the
//! listener is torn down when that scope's owner is cleaned up.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use leptos::prelude::*;

/// Register a bubble-phase `window` listener, owned by the current reactive
/// owner. The wasm-bindgen `Closure` is parked in a `StoredValue` for the
/// owner's lifetime and removed before it is dropped on cleanup (the same
/// registration pattern as [`super::dom`]'s observers).
pub fn use_window_event(event: &'static str, handler: impl Fn(web_sys::Event) + 'static) {
    add_owned(event, false, handler);
}

/// Add a capture-phase listener on `window`, owned by the current reactive
/// owner. Scroll events do not bubble, but they DO hit window listeners in the
/// capture phase, so one such listener catches `#page-list`, the single-page
/// container and the window alike.
///
/// Must be called from inside a reactive scope (an `Effect`); the listener is
/// torn down when that scope's owner is cleaned up.
pub fn add_window_capture_listener(event: &str, handler: impl FnMut(web_sys::Event) + 'static) {
    add_owned(event, true, handler);
}

/// The registration both flavours share.
///
/// Dropping a `Closure` frees the JS shim it handed the browser, so the closure
/// has to outlive every dispatch it is queued for: it is parked in a
/// `StoredValue` for the owner's lifetime, and cleanup removes the listener
/// from the window BEFORE the park is released. Removal has to name the same
/// capture phase the registration used, or the browser keeps the listener and
/// the shim goes away underneath it — hence one `capture` flag driving both
/// calls instead of two functions that could drift apart.
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
