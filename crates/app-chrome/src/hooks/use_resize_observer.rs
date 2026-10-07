//! ResizeObserver plumbing: one install, one teardown, no closure leaks.

use std::rc::Rc;

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::ResizeObserverEntry;

use super::dom::by_id;

/// One installed observer and the wasm closure keeping it alive.
type ObserverSlot = (
    StoredValue<Option<web_sys::ResizeObserver>, LocalStorage>,
    StoredValue<Option<Closure<dyn FnMut(Vec<ResizeObserverEntry>)>>, LocalStorage>,
);

fn new_slot() -> ObserverSlot {
    (
        StoredValue::new_local(None::<web_sys::ResizeObserver>),
        StoredValue::new_local(None::<Closure<dyn FnMut(Vec<ResizeObserverEntry>)>>),
    )
}

/// Disconnect an installed observer and drop its closure, in that order.
fn stop_observing(slot: ObserverSlot) {
    if let Some(observer) = slot.0.try_get_value().flatten() {
        observer.disconnect();
    }
    let _ = slot.0.try_set_value(None);
    let _ = slot.1.try_set_value(None);
}

/// Observe `elements` into `slot`, replacing any previous install.
fn install_observer(
    slot: ObserverSlot,
    elements: &[web_sys::Element],
    on_resize: &Rc<dyn Fn(Vec<ResizeObserverEntry>)>,
) {
    stop_observing(slot);
    let on_resize = Rc::clone(on_resize);
    let callback: Closure<dyn FnMut(Vec<ResizeObserverEntry>)> =
        Closure::wrap(Box::new(move |entries: Vec<ResizeObserverEntry>| {
            on_resize(entries);
        }) as Box<dyn FnMut(Vec<ResizeObserverEntry>)>);
    let fn_ref: &js_sys::Function = callback.as_ref().unchecked_ref();
    if let Ok(observer) = web_sys::ResizeObserver::new(fn_ref) {
        for el in elements {
            observer.observe(el);
        }
        slot.0.set_value(Some(observer));
        slot.1.set_value(Some(callback));
    }
}

/// Install one observer over `elements` for the current owner.
pub fn observe_elements(
    elements: Vec<web_sys::Element>,
    on_resize: impl Fn(Vec<ResizeObserverEntry>) + 'static,
) {
    let on_resize: Rc<dyn Fn(Vec<ResizeObserverEntry>)> = Rc::new(on_resize);
    let slot = new_slot();

    Effect::new(move || {
        install_observer(slot, &elements, &on_resize);
    });

    on_cleanup(move || stop_observing(slot));
}

/// Report an element's content-box size into `sink`.
pub fn observe_content_size(
    element_id: &'static str,
    sink: RwSignal<(f64, f64)>,
) -> impl Fn() + Send + Sync + 'static {
    observe_content_size_with(move || by_id(element_id), sink)
}

/// [`observe_content_size`] for an element the caller looks up itself.
pub fn observe_content_size_with(
    find: impl Fn() -> Option<web_sys::Element> + 'static,
    sink: RwSignal<(f64, f64)>,
) -> impl Fn() + Send + Sync + 'static {
    let slot = new_slot();
    Effect::new(move || {
        let Some(el) = find() else {
            return;
        };
        let on_resize: Rc<dyn Fn(Vec<ResizeObserverEntry>)> =
            Rc::new(move |entries: Vec<ResizeObserverEntry>| {
                if let Some(entry) = entries.first() {
                    let rect = entry.content_rect();
                    sink.set((rect.width(), rect.height()));
                }
            });
        install_observer(slot, std::slice::from_ref(&el), &on_resize);
    });
    move || stop_observing(slot)
}

/// Observe a `NodeRef` element, re-arming when the node changes.
pub fn use_resize_observer(
    target: NodeRef<html::Div>,
    on_resize: impl Fn(ResizeObserverEntry) + 'static,
) {
    let on_resize = Rc::new(on_resize);
    let observer_handle = StoredValue::new_local(None::<web_sys::ResizeObserver>);
    let callback_handle =
        StoredValue::new_local(None::<Closure<dyn FnMut(Vec<ResizeObserverEntry>)>>);
    let observed = StoredValue::new_local(None::<web_sys::Element>);

    Effect::new(move |_| {
        let Some(el) = target.get() else {
            return;
        };
        // Compare through the base Element type: an HtmlDivElement IS one.
        let el: web_sys::Element = el.unchecked_into::<web_sys::Element>();
        if callback_handle.with_value(|c| c.is_some()) {
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
