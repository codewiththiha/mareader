//! Pointer-drag mechanics: forward `pointermove` coordinates while
//! `active`, ending on up or cancel.

use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

/// Track a drag on `window` while `active` is true.
pub fn use_pointer_drag(
    active: RwSignal<bool>,
    on_move: impl Fn(f64, f64) + 'static,
    on_end: impl Fn() + 'static,
) {
    let on_move = Rc::new(on_move);
    let on_end = Rc::new(on_end);

    Effect::new(move |_| {
        if !active.get() {
            return;
        }

        let on_move = Rc::clone(&on_move);
        let mv = window_event_listener_untyped("pointermove", move |ev: web_sys::Event| {
            let me = ev.unchecked_ref::<web_sys::MouseEvent>();
            on_move(me.client_x() as f64, me.client_y() as f64);
        });

        // Clone into each listener: the Effect closure must stay `FnMut`.
        let on_end_up = Rc::clone(&on_end);
        let up = window_event_listener_untyped("pointerup", move |_| on_end_up());
        let on_end_cancel = Rc::clone(&on_end);
        let cancel = window_event_listener_untyped("pointercancel", move |_| on_end_cancel());

        on_cleanup(move || {
            mv.remove();
            up.remove();
            cancel.remove();
        });
    });
}
