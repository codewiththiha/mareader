//! One registration path for app-lifetime Tauri event listeners.
//!
//! Every Tauri subscription used to hand-roll the same three-step ritual: wrap
//! the handler in a `Closure`, clone it as a `js_sys::Function` for the
//! engine's `listen` bridge, and park the closure in a `StoredValue` so the
//! listener stays registered. Parking is load-bearing: dropping the Rust-side
//! `Closure` frees the wasm function table entry while Tauri's JS still holds
//! a reference, and the next emitted event would call into freed memory.
//!
//! `unlisten_all` runs from dispose, before `release()` clears `wasm`. A
//! listener left behind calls into that cleared instance on the next resize
//! and the page that just mounted draws nothing.

use std::cell::RefCell;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use web_sys::Event;

struct Listeners {
    /// Set by dispose. A listen that resolves after that unlistens immediately
    /// instead of subscribing a module that is going away.
    closed: bool,
    handles: Vec<js_sys::Function>,
}

thread_local! {
    static LISTENERS: RefCell<Listeners> = const {
        RefCell::new(Listeners {
            closed: false,
            handles: Vec::new(),
        })
    };
}

/// Subscribe to a Tauri event until [`unlisten_all`]. Must run inside a
/// reactive owner: that owner keeps the parked closure alive until dispose.
pub fn tauri_listen(event: &str, handler: impl FnMut(Event) + 'static) {
    let cb = Closure::wrap(Box::new(handler) as Box<dyn FnMut(Event)>);
    let function: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
    let event = event.to_string();
    // Dispose also calls this. Registering it here keeps the function live in
    // every build: the wasm-only dispose sites are not compiled for host tests.
    leptos::prelude::on_cleanup(unlisten_all);
    wasm_bindgen_futures::spawn_local(async move {
        match tauri_bridge::listen(&event, function).await {
            Ok(value) => park_unlisten(value),
            Err(error) => {
                web_sys::console::error_2(
                    &JsValue::from_str(&format!("Could not listen for Tauri event '{event}'")),
                    &error,
                );
            }
        }
    });
    // Park the closure in the current owner: dropping it would free the wasm
    // function table entry while Tauri's JS still holds a reference.
    let _parked = leptos::prelude::StoredValue::new_local(Some(cb));
}

fn park_unlisten(value: JsValue) {
    let Ok(fun) = value.dyn_into::<js_sys::Function>() else {
        return;
    };
    // The boot reads this array from the frame that owns the listener and
    // calls it before that frame is removed. A handle that lives only in this
    // instance cannot be reached once `release()` has cleared `wasm`.
    #[cfg(target_arch = "wasm32")]
    remember_js_unlisten(&fun);
    let late = LISTENERS.with(|slot| {
        let mut listeners = slot.borrow_mut();
        if listeners.closed {
            Some(fun)
        } else {
            listeners.handles.push(fun);
            None
        }
    });
    if let Some(fun) = late {
        let _ = fun.call0(&JsValue::UNDEFINED);
    }
}

#[cfg(target_arch = "wasm32")]
fn remember_js_unlisten(fun: &js_sys::Function) {
    let Some(win) = web_sys::window() else {
        return;
    };
    let Ok(list) = js_sys::Reflect::get(&win, &JsValue::from_str("__MAREADER_UNLISTEN")) else {
        return;
    };
    let Ok(list) = list.dyn_into::<js_sys::Array>() else {
        return;
    };
    list.push(fun);
}

/// Drop every listener this instance registered. Dispose calls this while
/// `wasm` is still set. A second call is a no-op.
pub fn unlisten_all() {
    let pending = LISTENERS.with(|slot| {
        let mut listeners = slot.borrow_mut();
        listeners.closed = true;
        std::mem::take(&mut listeners.handles)
    });
    for fun in pending {
        let _ = fun.call0(&JsValue::UNDEFINED);
    }
}
