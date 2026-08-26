//! Per-item measurement: one shared `ResizeObserver` for every mounted item
//! element. Entries carry their index in `data-vl-index`; sizes are queued into
//! the core and flushed once per animation frame.

use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::{ResizeObserver, ResizeObserverEntry};

use crate::options::Axis;
use crate::virtualizer::VirtualizerInner;

/// Create (lazily) the shared item observer and observe `el` for `index`.
pub(crate) fn observe_item(inner: &Rc<VirtualizerInner>, index: usize, el: &web_sys::Element) {
    let _ = el.set_attribute("data-vl-index", &index.to_string());
    if inner.item_ro.borrow().is_none() {
        install(inner);
    }
    if let Some((ro, _)) = inner.item_ro.borrow().as_ref() {
        ro.observe(el);
    }
}

fn install(inner: &Rc<VirtualizerInner>) {
    let adapter = inner.clone();
    let cb = Closure::<dyn FnMut(js_sys::Array, ResizeObserver)>::new(
        move |entries: js_sys::Array, _| {
            for entry in entries.iter() {
                let entry: ResizeObserverEntry = entry.unchecked_into();
                let target = entry.target();
                let Some(target) = target.dyn_ref::<web_sys::Element>() else {
                    continue;
                };
                let Some(index) = target
                    .get_attribute("data-vl-index")
                    .and_then(|s| s.parse::<usize>().ok())
                else {
                    continue;
                };
                let rect = entry.content_rect();
                let size = match adapter.options.axis {
                    Axis::Vertical => rect.height(),
                    Axis::Horizontal => rect.width(),
                };
                adapter.core.borrow_mut().queue_size(index, size);
            }
            adapter.arm_flush();
        },
    );
    if let Ok(ro) = ResizeObserver::new(cb.as_ref().unchecked_ref()) {
        *inner.item_ro.borrow_mut() = Some((ro, cb));
    }
}
