//! One registration path for app-lifetime Tauri event listeners.
//!
//! Every Tauri subscription used to hand-roll the same three-step ritual: wrap
//! the handler in a `Closure`, clone it as a `js_sys::Function` for the
//! engine's `listen` bridge, and park the closure in a `StoredValue` so the
//! listener stays registered. Parking is load-bearing: dropping the Rust-side
//! `Closure` frees the wasm function table entry while Tauri's JS still holds
//! a reference, and the next emitted event would call into freed memory. This
//! helper owns that ritual once.

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::Event;

/// Subscribe to a Tauri event for the lifetime of the surrounding reactive
/// owner, and unsubscribe when that owner is disposed.
///
/// The unsubscribe is load-bearing now that a runtime frame outlives its
/// sessions (the Shell recycles a frame instead of rebuilding it): Tauri's
/// listener registry lives in the host window, so a session that ended
/// without unlistening would leave Tauri holding a handler whose closure
/// was freed with the session — every later event would call into a dropped
/// closure, and each recycled session would add one more dead listener.
/// Must run inside a reactive owner: that owner keeps the closure alive and
/// its cleanup is the unlisten.
pub fn tauri_listen(event: &str, handler: impl FnMut(Event) + 'static) {
    // Scoped here: the prelude's value traits (`try_update_value`) are what
    // this needs, and a module-level glob would shadow `web_sys::Event`.
    use leptos::prelude::{StoredValue, UpdateValue, on_cleanup};
    // A plain browser has no `window.__TAURI__`: the async listen extern
    // would throw the moment its import shim ran, and a throw inside a
    // Leptos task aborts the whole task drain — every later spawn_local
    // queued behind it (the document open among them) would never run.
    // Tauri events simply do not exist off the webview, so subscribing is
    // a no-op there.
    if !tauri_bridge::has_tauri() {
        return;
    }
    let cb = Closure::wrap(Box::new(handler) as Box<dyn FnMut(Event)>);
    let f: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
    // The closure and the unlisten handle share one owner-scoped slot: the
    // closure must outlive Tauri's reference to it, so it is only dropped
    // after the unlisten has run.
    let slot = StoredValue::new_local(Subscription {
        closure: Some(cb),
        unlisten: None,
    });
    on_cleanup(move || {
        let _ = slot.try_update_value(Subscription::release);
    });
    let event = event.to_string();
    wasm_bindgen_futures::spawn_local(async move {
        match tauri_bridge::listen(&event, f).await {
            Ok(unlisten) => {
                let Ok(unlisten) = unlisten.dyn_into::<js_sys::Function>() else {
                    return;
                };
                // The owner may already be gone (a session disposed while
                // its subscription was still in flight): then nobody will
                // ever unlisten, so do it on arrival.
                let kept = slot.try_update_value(|sub| {
                    if sub.closure.is_some() {
                        sub.unlisten = Some(unlisten.clone());
                        true
                    } else {
                        false
                    }
                });
                if kept != Some(true) {
                    let _ = unlisten.call0(&wasm_bindgen::JsValue::NULL);
                }
            }
            Err(error) => {
                web_sys::console::error_2(
                    &wasm_bindgen::JsValue::from_str(&format!(
                        "Could not listen for Tauri event '{event}'"
                    )),
                    &error,
                );
            }
        }
    });
}

/// One live subscription: the handler's closure and, once Tauri answered,
/// the function that removes it.
struct Subscription {
    closure: Option<Closure<dyn FnMut(Event)>>,
    unlisten: Option<js_sys::Function>,
}

impl Subscription {
    /// Unlisten first, then free the closure — never the other order.
    fn release(&mut self) {
        if let Some(unlisten) = self.unlisten.take() {
            let _ = unlisten.call0(&wasm_bindgen::JsValue::NULL);
        }
        self.closure = None;
    }
}
