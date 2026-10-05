//! One owner-scoped native event subscription. Pending registration and
//! retirement share the same state, so unlisten always precedes closure
//! release even when a route is disposed before Tauri answers.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::Event;

/// Subscribe to a Tauri event for the lifetime of the surrounding reactive
/// owner, and unsubscribe when that owner is disposed.
///
/// A listener being registered may already be held by the parent window.
/// Retirement makes its handler inert immediately, but retains the closure
/// until the registration answer can be unlistened. No dropped WASM callback
/// remains in the native registry.
pub fn tauri_listen(event: &str, mut handler: impl FnMut(Event) + 'static) {
    // Scoped here: the prelude's value traits (`try_update_value`) are what
    // this needs, and a module-level glob would shadow `web_sys::Event`.
    use leptos::prelude::{StoredValue, WithValue, on_cleanup};
    // A plain browser has no `window.__TAURI__`: the async listen extern
    // would throw the moment its import shim ran, and a throw inside a
    // Leptos task aborts the whole task drain — every later spawn_local
    // queued behind it (the document open among them) would never run.
    // Tauri events simply do not exist off the webview, so subscribing is
    // a no-op there.
    if !tauri_bridge::has_tauri() {
        return;
    }
    let alive = Rc::new(Cell::new(true));
    let active = alive.clone();
    let cb = Closure::wrap(Box::new(move |event| {
        if active.get() {
            handler(event);
        }
    }) as Box<dyn FnMut(Event)>);
    let f: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
    let subscription = Rc::new(RefCell::new(Subscription {
        alive,
        closure: Some(cb),
        unlisten: None,
        registering: true,
    }));
    let slot = StoredValue::new_local(subscription.clone());
    on_cleanup(move || {
        let _ = slot.try_with_value(|sub| sub.borrow_mut().retire());
    });
    let event = event.to_string();
    wasm_bindgen_futures::spawn_local(async move {
        match tauri_bridge::listen(&event, f).await {
            Ok(unlisten) => {
                let mut sub = subscription.borrow_mut();
                sub.registering = false;
                sub.unlisten = unlisten.dyn_into::<js_sys::Function>().ok();
                if !sub.alive.get() {
                    sub.release();
                }
            }
            Err(error) => {
                let mut sub = subscription.borrow_mut();
                sub.registering = false;
                if sub.alive.get() {
                    web_sys::console::error_2(
                        &wasm_bindgen::JsValue::from_str(&format!(
                            "Could not listen for Tauri event '{event}'"
                        )),
                        &error,
                    );
                }
                sub.release();
            }
        }
    });
}

/// One live subscription: the handler's closure and, once Tauri answered,
/// the function that removes it.
struct Subscription {
    alive: Rc<Cell<bool>>,
    registering: bool,
    closure: Option<Closure<dyn FnMut(Event)>>,
    unlisten: Option<js_sys::Function>,
}

impl Subscription {
    fn retire(&mut self) {
        self.alive.set(false);
        if !self.registering {
            self.release();
        }
    }

    /// Unlisten first, then free the closure — never the other order.
    fn release(&mut self) {
        if let Some(unlisten) = self.unlisten.take() {
            let _ = unlisten.call0(&wasm_bindgen::JsValue::NULL);
        }
        self.closure = None;
    }
}
