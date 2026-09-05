//! ResizeObserver plumbing: one install, one teardown, no closure leaks.
//!
//! `DocumentTitle`, the thumbnail panel and the gloss card's content-size hook
//! each carried an identical ~45-line block for this: two `StoredValue`s, a run-once guard, a
//! `Closure::wrap`, a `ResizeObserver`, and an `on_cleanup` that MUST
//! disconnect before the closure is dropped. Only the observed elements
//! differed.
//!
//! The disconnect is load-bearing, not tidiness: unmounting the view removes
//! the observed element, which queues a resize notification into a closure
//! that is about to be freed. Without the explicit `disconnect()` the wasm
//! runtime aborts with "closure invoked recursively or after being dropped".

use std::rc::Rc;

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::ResizeObserverEntry;

use super::dom::by_id;

/// Install one observer over the given elements for the current reactive
/// owner, forwarding every callback batch to `on_resize` (the browser already
/// coalesces the notifications).
pub fn observe_elements(
    elements: Vec<web_sys::Element>,
    on_resize: impl Fn(Vec<ResizeObserverEntry>) + 'static,
) {
    let on_resize = Rc::new(on_resize);
    let observer_handle = StoredValue::new_local(None::<web_sys::ResizeObserver>);
    let callback_handle = StoredValue::new_local(None::<Closure<dyn FnMut(Vec<ResizeObserverEntry>)>>);

    Effect::new(move || {
        // Guard: only set up once (StoredValue access is non-reactive).
        if callback_handle.with_value(|c| c.is_some()) {
            return;
        }
        let on_resize = Rc::clone(&on_resize);
        let callback: Closure<dyn FnMut(Vec<ResizeObserverEntry>)> =
            Closure::wrap(Box::new(move |entries: Vec<ResizeObserverEntry>| {
                on_resize(entries);
            }) as Box<dyn FnMut(Vec<ResizeObserverEntry>)>);
        let fn_ref: &js_sys::Function = callback.as_ref().unchecked_ref();
        if let Ok(observer) = web_sys::ResizeObserver::new(fn_ref) {
            for el in &elements {
                observer.observe(el);
            }
            observer_handle.set_value(Some(observer));
            callback_handle.set_value(Some(callback));
        }
    });

    on_cleanup(move || {
        if let Some(observer) = observer_handle.try_get_value().flatten() {
            observer.disconnect();
        }
        let _ = observer_handle.try_set_value(None);
        let _ = callback_handle.try_set_value(None);
    });
}

/// Report an element's content-box size into `sink` for as long as the calling
/// view is mounted (looked up by id; see [`super::dom::by_id`]).
pub fn observe_content_size(element_id: &'static str, sink: RwSignal<(f64, f64)>) {
    let sink_arm = sink;
    Effect::new(move || {
        let Some(el) = by_id(element_id) else {
            return;
        };
        observe_elements(vec![el], move |entries| {
            if let Some(entry) = entries.first() {
                let rect = entry.content_rect();
                sink_arm.set((rect.width(), rect.height()));
            }
        });
    });
}

/// Observe a `NodeRef` element and forward each resize entry to `on_resize`.
/// Re-arms when the node identity changes (remounts create a fresh element).
pub fn use_resize_observer(target: NodeRef<html::Div>, on_resize: impl Fn(ResizeObserverEntry) + 'static) {
    let on_resize = Rc::new(on_resize);
    let observer_handle = StoredValue::new_local(None::<web_sys::ResizeObserver>);
    let callback_handle = StoredValue::new_local(None::<Closure<dyn FnMut(Vec<ResizeObserverEntry>)>>);
    let observed = StoredValue::new_local(None::<web_sys::Element>);

    Effect::new(move |_| {
        let Some(el) = target.get() else {
            return;
        };
        // Compare/observe through the base Element type (the NodeRef is typed
        // as Div; the observer takes web_sys::Element). Unchecked is sound:
        // an HtmlDivElement *is* an Element (same JS object), just viewed
        // through the base interface.
        let el: web_sys::Element = el.unchecked_into::<web_sys::Element>();
        if callback_handle.with_value(|c| c.is_some()) {
            // Same node still mounted: nothing to do.
            if observed.with_value(|o| o.as_ref().is_some_and(|o| o == &el)) {
                return;
            }
            // Node replaced: disconnect before reinstalling.
            if let Some(observer) = observer_handle.try_get_value().flatten() {
                observer.disconnect();
            }
            callback_handle.try_set_value(None);
            observer_handle.try_set_value(None);
        }
        let on_resize = Rc::clone(&on_resize);
        let callback: Closure<dyn FnMut(Vec<ResizeObserverEntry>)> =
            Closure::wrap(Box::new(move |entries: Vec<ResizeObserverEntry>| {
                if let Some(entry) = entries.first() {
                    on_resize(entry.clone());
                }
            }) as Box<dyn FnMut(Vec<ResizeObserverEntry>)>);
        let fn_ref: &js_sys::Function = callback.as_ref().unchecked_ref();
        if let Ok(observer) = web_sys::ResizeObserver::new(fn_ref) {
            observer.observe(&el);
            observer_handle.set_value(Some(observer));
            callback_handle.set_value(Some(callback));
            observed.set_value(Some(el));
        }
    });

    on_cleanup(move || {
        if let Some(observer) = observer_handle.try_get_value().flatten() {
            observer.disconnect();
        }
        let _ = observer_handle.try_set_value(None);
        let _ = callback_handle.try_set_value(None);
    });
}
