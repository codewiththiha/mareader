//! One registration path for app-lifetime Tauri event listeners.
//!
//! Every Tauri subscription in the app used to hand-roll the same three-step
//! ritual: wrap the handler in a `Closure`, clone it as a `js_sys::Function`
//! for the engine's `listen` bridge, and park the closure in a `StoredValue`
//! so the listener stays registered. Parking is load-bearing, not ceremony —
//! dropping the Rust-side `Closure` frees the wasm function table entry while
//! Tauri's JS still holds a reference to it, and the next emitted event would
//! call into freed memory. This helper owns that ritual once.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::Event;

/// Subscribe to a Tauri event for the lifetime of the surrounding reactive
/// owner. The unlisten handle is deliberately discarded — Tauri keeps the
/// listener registered until it is called, and no app surface ever unsubscribes.
///
/// Must run inside a reactive owner (every caller today installs from the app
/// root or a long-lived shell component), because that owner is what keeps the
/// parked closure alive.
pub fn tauri_listen(event: &str, handler: impl FnMut(Event) + 'static) {
    let cb = Closure::wrap(Box::new(handler) as Box<dyn FnMut(Event)>);
    let f: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
    let event = event.to_string();
    wasm_bindgen_futures::spawn_local(async move {
        _ = pdf_engine::listen(&event, f).await;
    });
    // Park the closure in the current owner: dropping it would free the wasm
    // function table entry while Tauri's JS still holds a reference.
    let _parked = leptos::prelude::StoredValue::new_local(Some(cb));
}
