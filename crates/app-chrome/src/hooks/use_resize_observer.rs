//! ResizeObserver plumbing: one install, one teardown, no closure leaks. Three
//! consumers each carried an identical ~45-line block (two `StoredValue`s, a
//! run-once guard, a `Closure::wrap`, the observer, and an `on_cleanup` that
//! MUST disconnect before the closure is dropped); only the observed elements
//! differed.
//!
//! The disconnect is load-bearing: unmounting removes the observed element,
//! which queues a resize notification into a closure about to be freed —
//! without the explicit `disconnect()` the wasm runtime aborts with "closure
//! invoked recursively or after being dropped".

use std::rc::Rc;

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::ResizeObserverEntry;

use super::dom::by_id;

/// One installed observer: the observer itself, and the wasm closure keeping
/// its JS callback alive. Two `StoredValue` slots, not one pair, because the
/// closure is not `Clone` and the read side of a slot (`try_get_value`)
/// requires it — the observer slot answers "what is connected", the closure
/// slot only ever gets swapped out, which hands the old closure back for the
/// drop. Both handles are `Copy` and `Send + Sync`, so a teardown can reach
/// them from any owner — including a `stop` handle the installer hands out.
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

/// Disconnect an installed observer and drop its closure. The closure goes
/// AFTER the disconnect: dropping it first would leave the observer holding
/// a dangling JS callback. Idempotent, and safe against a slot whose arena
/// item is already gone (`try_*`).
fn stop_observing(slot: ObserverSlot) {
    if let Some(observer) = slot.0.try_get_value().flatten() {
        observer.disconnect();
    }
    let _ = slot.0.try_set_value(None);
    let _ = slot.1.try_set_value(None);
}

/// Observe `elements` into `slot`, replacing any previous install. The
/// replace-first is what makes an effect re-run safe: the old observer is
/// disconnected before the new one exists, so a node identity change can
/// never leave two observers (or two retained closures) on the tree.
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

/// Install one observer over the given elements for the current reactive
/// owner, forwarding every callback batch to `on_resize` (the browser already
/// coalesces the notifications). The disconnect rides this owner's cleanup.
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

/// Report an element's content-box size into `sink` for as long as the caller
/// holds the returned stopper (looked up by id; see [`super::dom::by_id`]).
///
/// The teardown is the CALLER's, explicitly: register the returned function
/// with `on_cleanup` where the observation is mounted. Letting the disconnect
/// ride the install effect's own owner made the observer's lifetime a fact
/// about effect-disposal order instead — and an observer that outlives its
/// shell retains the observed element, which for a page scroller means every
/// canvas ever mounted inside it.
pub fn observe_content_size(
    element_id: &'static str,
    sink: RwSignal<(f64, f64)>,
) -> impl Fn() + Send + Sync + 'static {
    let slot = new_slot();
    Effect::new(move || {
        let Some(el) = by_id(element_id) else {
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
        // Div; the observer takes web_sys::Element). Unchecked is sound: an
        // HtmlDivElement IS an Element — the same JS object through the base
        // interface.
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
