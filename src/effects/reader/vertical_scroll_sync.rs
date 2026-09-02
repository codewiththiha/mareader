//! Continuous-scroll effect: maps container scroll -> viewer.scroll_top.

//!
//! Runs once when the continuous view mounts. Attaches a scroll listener to
//! `#page-list` and writes `viewer.scroll_top` whenever the container offset
//! moves by >= 0.5px (a cheap rAF-style throttle — Leptos signals only notify
//! on actual value change, so this just avoids redundant JS->Rust calls).
//! The listener is removed when the view unmounts.
//!
//! NO POINTERDOWN FOCUS STEALER. Focusing `#page-list` on every click inside
//! the scroller interrupts the browser's selection initialization: when the
//! reader clicks-and-drags to select text, the focus steal fires on
//! pointerdown, stealing focus from the text-layer span the click started on,
//! and the browser's drag-selection tracking was disrupted — multi-page
//! selection was discontinuous and jumpy. Removing the pointerdown listener
//! lets the browser own selection initialization; keyboard focus recovery on
//! virtualization unmount (focusout) is also removed because it caused the same
//! class of interference.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;

type CleanupSlot = leptos::prelude::StoredValue<
    Option<(
        web_sys::Element,
        Vec<(&'static str, js_sys::Function)>,
    )>,
>;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::Closure;
use wasm_bindgen::JsCast;
use web_sys::Event;

use crate::state::ReaderState;
use app_chrome::hooks::dom::page_list;

/// The wasm-side wrapper around the JS scroll handler. Not `Send + Sync`, so it
/// can't be captured by `on_cleanup` — it is parked in a local `StoredValue`
/// that lives as long as this view's reactive owner.
type ScrollListener = Closure<dyn FnMut(Event)>;

/// Must be called once when the continuous view mounts.
pub fn vertical_scroll_sync(state: ReaderState) {
    let scroll_top = state.viewer.scroll_top;

    // The (element, JS function) pair is parked here so `on_cleanup` (which
    // requires `Send + Sync`) can detach the listener on unmount.
    let cleanup_slot: CleanupSlot = StoredValue::new(None);

    // Holds the wasm Closure alive for the lifetime of this view. Dropped when
    // the component's reactive owner is disposed — which happens *after*
    // `on_cleanup` runs, so the listener is already detached by then.
    let listener_slot: StoredValue<Option<ScrollListener>, _> = StoredValue::new_local(None);

    on_cleanup(move || {
        if let Some((el, listeners)) = cleanup_slot.get_value() {
            for (event, cb) in &listeners {
                _ = el.remove_event_listener_with_callback(event, cb);
            }
        }
    });

    spawn_local(async move {
        let Some(el) = page_list()
        else {
            return;
        };

        // Restore a previously-saved offset (e.g. switching back to continuous).
        let saved = scroll_top.get();
        if saved > 0.0 {
            el.set_scroll_top(saved as i32);
        }

        let last = Rc::new(Cell::new(f64::NAN));
        let handler = {
            let el = el.clone();
            move |_: Event| {
                let y = el.scroll_top() as f64;
                let prev = last.get();
                // Only write when the offset actually moved by >= 0.5px.
                if prev.is_nan() || (y - prev).abs() >= 0.5 {
                    last.set(y);
                    scroll_top.set(y);
                }
            }
        };

        let closure = Closure::new(handler);
        let cb: js_sys::Function = closure.as_ref().unchecked_ref::<js_sys::Function>().clone();
        if el.add_event_listener_with_callback("scroll", &cb).is_ok() {
            cleanup_slot.set_value(Some((
                el,
                vec![("scroll", cb)],
            )));
            listener_slot.set_value(Some(closure));
        }
    });
}
