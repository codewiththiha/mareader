//! Owner-scoped Tauri subscriptions, including disposal during registration.
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::Event;

pub fn tauri_listen(event: &str, handler: impl FnMut(Event) + 'static) {
    let callback = Rc::new(Closure::wrap(Box::new(handler) as Box<dyn FnMut(Event)>));
    let function: js_sys::Function = callback.as_ref().as_ref().unchecked_ref::<js_sys::Function>().clone();
    let disposed = Rc::new(Cell::new(false));
    let unlisten = Rc::new(RefCell::new(None::<js_sys::Function>));
    let parked = StoredValue::new_local((callback.clone(), disposed.clone(), unlisten.clone()));
    on_cleanup(move || {
        parked.with_value(|(_, disposed, unlisten)| {
            disposed.set(true);
            if let Some(unlisten) = unlisten.borrow_mut().take() {
                let _ = unlisten.call0(&JsValue::UNDEFINED);
            }
        });
    });
    let event = event.to_string();
    wasm_bindgen_futures::spawn_local(async move {
        // Keep the Closure alive until registration resolves, even if its
        // owner has already gone. A late registration is immediately undone.
        let _callback = callback;
        match tauri_bridge::listen(&event, function).await {
            Ok(value) => {
                if let Ok(handle) = value.dyn_into::<js_sys::Function>() {
                    if disposed.get() {
                        let _ = handle.call0(&JsValue::UNDEFINED);
                    } else {
                        *unlisten.borrow_mut() = Some(handle);
                    }
                }
            }
            Err(error) => web_sys::console::error_2(
                &JsValue::from_str(&format!("Could not listen for Tauri event '{event}'")), &error),
        }
    });
}
