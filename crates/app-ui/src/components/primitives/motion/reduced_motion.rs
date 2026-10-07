//! `prefers-reduced-motion` helpers: a snapshot and a reactive signal.

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use leptos::prelude::*;

/// True when the OS asks for reduced motion (a non-reactive snapshot).
pub fn prefers_reduced_motion() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok())
        .flatten()
        .map(|m| m.matches())
        .unwrap_or(false)
}

/// A reactive `prefers-reduced-motion` signal, kept in sync by a
/// `change` listener.
pub fn reduced_motion_signal() -> RwSignal<bool> {
    let s = RwSignal::new(prefers_reduced_motion());
    let mql_store = StoredValue::new_local(None::<web_sys::MediaQueryList>);
    let cb_store = StoredValue::new_local(None::<Closure<dyn FnMut()>>);
    let f_store = StoredValue::new_local(None::<js_sys::Function>);
    Effect::new(move |_| {
        // Once only: a second registration would strand the first listener.
        if mql_store.try_get_value().flatten().is_some() {
            return;
        }
        let Some(mql) = web_sys::window()
            .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok())
            .flatten()
        else {
            return;
        };
        let mql_for_cb = mql.clone();
        let cb: Closure<dyn FnMut()> = Closure::new(move || {
            // The listener fires outside any owner: no disposed writes.
            let _ = s.try_set(mql_for_cb.matches());
        });
        let f: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
        let _ = mql.add_event_listener_with_callback("change", &f);
        let _ = mql_store.try_set_value(Some(mql));
        let _ = cb_store.try_set_value(Some(cb));
        let _ = f_store.try_set_value(Some(f));
    });
    on_cleanup(move || {
        if let (Some(mql), Some(f)) = (
            mql_store.try_get_value().flatten(),
            f_store.try_get_value().flatten(),
        ) {
            let _ = mql.remove_event_listener_with_callback("change", &f);
        }
        let _ = cb_store.try_set_value(None);
    });
    s
}
