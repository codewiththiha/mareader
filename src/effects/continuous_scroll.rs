//! Continuous-scroll effect: maps container scroll -> viewer.scroll_top.
//! OWNED BY branch A (viewer/continuous).
//!
//! Runs once when the continuous view mounts. Attaches a scroll listener to
//! `#page-list` and writes `viewer.scroll_top` whenever the container offset
//! moves by >= 0.5px (a cheap rAF-style throttle — Leptos signals only notify
//! on actual value change, so this just avoids redundant JS->Rust calls).
//! The listener is removed when the view unmounts.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::Closure;
use wasm_bindgen::JsCast;
use web_sys::Event;

use crate::core::state::AppState;
use crate::util::dom::page_list;

/// The wasm-side wrapper around the JS scroll handler. Not `Send + Sync`, so it
/// can't be captured by `on_cleanup` — it is parked in a local `StoredValue`
/// that lives as long as this view's reactive owner.
type ScrollListener = Closure<dyn FnMut(Event)>;

/// Must be called once when the continuous view mounts.
pub fn continuous_scroll(state: AppState) {
    let scroll_top = state.viewer.scroll_top;

    // The component body runs before the DOM is mounted, so the `#page-list`
    // lookup is deferred to a microtask. The (element, JS function) pair is
    // parked here so `on_cleanup` (which requires `Send + Sync`) can detach the
    // listener on unmount. The JS `Function` handle is independent of the wasm
    // `Closure`, so removal is safe even though the Closure itself is !Send.
    let cleanup_slot: StoredValue<Option<(web_sys::Element, js_sys::Function)>> =
        StoredValue::new(None);

    // Holds the wasm Closure alive for the lifetime of this view. Dropped when
    // the component's reactive owner is disposed — which happens *after*
    // `on_cleanup` runs, so the listener is already detached by then.
    let listener_slot: StoredValue<Option<ScrollListener>, _> = StoredValue::new_local(None);
    let extra_slots: StoredValue<Option<(ScrollListener, ScrollListener)>, _> =
        StoredValue::new_local(None);

    on_cleanup(move || {
        if let Some((el, cb)) = cleanup_slot.get_value() {
            let _ = el.remove_event_listener_with_callback("scroll", &cb);
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
            let _ = el.set_scroll_top(saved as i32);
        }

        // Keep keyboard focus on the scroller itself. Arrow-key repeat is
        // targeted at the focused node; if that node is a text-layer span
        // on a page the virtualizer then unmounts, the repeat dies and the
        // next press lands on <body>, which is not this container. Focusing
        // `#page-list` (tabindex=0) on pointerdown, and reclaiming focus
        // when a descendant is removed (focusout with no relatedTarget),
        // keeps the keys aimed at a node that outlives any one page.
        let focus_list = {
            let el = el.clone();
            move || {
                if let Some(html) = el.dyn_ref::<web_sys::HtmlElement>() {
                    let opts = web_sys::FocusOptions::new();
                    opts.set_prevent_scroll(true);
                    let _ = html.focus_with_options(&opts);
                }
            }
        };
        let on_pointer = {
            let focus_list = focus_list.clone();
            move |_: Event| focus_list()
        };
        let on_focusout = {
            let el = el.clone();
            let focus_list = focus_list.clone();
            move |ev: Event| {
                let related = ev
                    .dyn_ref::<web_sys::FocusEvent>()
                    .and_then(|e| e.related_target());
                // relatedTarget is null when the focused node was removed
                // (virtualization). A click elsewhere sets it to the new
                // target — do not steal that.
                if related.is_none() {
                    let still_inside = web_sys::window()
                        .and_then(|w| w.document())
                        .and_then(|d| d.active_element())
                        .and_then(|a| a.dyn_into::<web_sys::Node>().ok())
                        .map(|n| el.contains(Some(&n)))
                        .unwrap_or(false);
                    if !still_inside {
                        focus_list();
                    }
                }
            }
        };
        let ptr_closure: Closure<dyn FnMut(Event)> = Closure::new(on_pointer);
        let fo_closure: Closure<dyn FnMut(Event)> = Closure::new(on_focusout);
        let ptr_cb: js_sys::Function =
            ptr_closure.as_ref().unchecked_ref::<js_sys::Function>().clone();
        let fo_cb: js_sys::Function =
            fo_closure.as_ref().unchecked_ref::<js_sys::Function>().clone();
        let _ = el.add_event_listener_with_callback("pointerdown", &ptr_cb);
        let _ = el.add_event_listener_with_callback("focusout", &fo_cb);
        // Park the extra Closures next to the scroll listener so they live
        // for the view's lifetime. The scroll cleanup only removes "scroll";
        // these die with the owner, which is fine — the element is gone.
        let _ = (ptr_closure, fo_closure);

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
            cleanup_slot.set_value(Some((el, cb)));
            listener_slot.set_value(Some(closure));
        }
    });
}
