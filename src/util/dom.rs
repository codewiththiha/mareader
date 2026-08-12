//! Small DOM lookups shared by the effects and views.
//!
//! `#page-list` (the continuous-scroll container) was resolved by an inlined
//! `window -> document -> get_element_by_id` chain in nine places across five
//! modules, each repeating the same three `and_then`s and each free to
//! misspell the id. These helpers make the id a single constant and the lookup
//! a single expression.

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::ResizeObserverEntry;

/// Id of the continuous viewer's scroll container.
pub const PAGE_LIST_ID: &str = "page-list";

/// Id of the single-page view's container.
pub const SINGLE_PAGE_CONTAINER_ID: &str = "single-page-container";

/// The element with `id`, if the document is available and it exists.
pub fn by_id(id: &str) -> Option<web_sys::Element> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
}

/// The continuous viewer's scroll container, if it is mounted.
pub fn page_list() -> Option<web_sys::Element> {
    by_id(PAGE_LIST_ID)
}

/// Report an element's content-box size into `sink` for as long as the calling
/// view is mounted.
///
/// `ContinuousView` and `SinglePageView` each carried an identical ~45-line
/// block for this: two `StoredValue`s, a run-once guard, a `Closure::wrap`, a
/// `ResizeObserver`, and an `on_cleanup` that MUST disconnect before the
/// closure is dropped. Only the element id differed. The observer and its
/// closure are parked in local `StoredValue`s so the JS references stay alive
/// for the view's lifetime.
///
/// The disconnect is load-bearing, not tidiness: unmounting the view removes
/// the observed element, which queues a resize notification into a closure
/// that is about to be freed. Without the explicit `disconnect()` the wasm
/// runtime aborts with "closure invoked recursively or after being dropped".
pub fn observe_content_size(element_id: &'static str, sink: RwSignal<(f64, f64)>) {
    let observer_handle = StoredValue::new_local(None::<web_sys::ResizeObserver>);
    let callback_handle =
        StoredValue::new_local(None::<Closure<dyn FnMut(Vec<ResizeObserverEntry>)>>);

    Effect::new(move || {
        // Guard: only set up once (StoredValue access is non-reactive).
        if callback_handle.with_value(|c| c.is_some()) {
            return;
        }
        let Some(el) = by_id(element_id) else {
            return;
        };
        let callback: Closure<dyn FnMut(Vec<ResizeObserverEntry>)> = Closure::wrap(Box::new(
            move |entries: Vec<ResizeObserverEntry>| {
                if let Some(entry) = entries.first() {
                    let rect = entry.content_rect();
                    sink.set((rect.width(), rect.height()));
                }
            },
        )
            as Box<dyn FnMut(Vec<ResizeObserverEntry>)>);
        let fn_ref: &js_sys::Function = callback.as_ref().unchecked_ref();
        if let Ok(observer) = web_sys::ResizeObserver::new(fn_ref) {
            observer.observe(&el);
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

/// Scroll `el`'s scroll parent so `el` is comfortably visible, but ONLY if it
/// is currently out of view.
///
/// WHY "only if out of view". The reader has two jobs here: following along as
/// the reader scrolls the document, and landing somewhere sensible when the
/// panel is opened. Unconditionally centring on every page change would yank
/// the list under the cursor while someone is reading down it — the row they
/// were about to click slides away. Scrolling only when the target is off
/// screen keeps the list still during normal browsing and still guarantees the
/// active row is reachable.
///
/// `margin` keeps the row off the very edge of the viewport, so there is
/// visible context above/below it rather than the row being flush against the
/// frame.
pub fn reveal_in_scroll_parent(el: &web_sys::Element, parent: &web_sys::Element, margin: f64) {
    let parent_h = parent.client_height() as f64;
    if parent_h <= 0.0 {
        return;
    }
    // offset_top is relative to the offset parent, which is not necessarily
    // the scroller, so measure through bounding rects instead — they share a
    // viewport origin and therefore always subtract correctly.
    let er = el.get_bounding_client_rect();
    let pr = parent.get_bounding_client_rect();
    let scroll_top = parent.scroll_top() as f64;

    // Position of the row within the scrollable content.
    let top = er.top() - pr.top() + scroll_top;
    let bottom = top + er.height();

    let view_top = scroll_top + margin;
    let view_bottom = scroll_top + parent_h - margin;

    let target = if top < view_top {
        // Above the fold: bring it to the top edge (plus margin).
        Some(top - margin)
    } else if bottom > view_bottom {
        // Below the fold: bring it to the bottom edge (minus margin).
        Some(bottom - parent_h + margin)
    } else {
        None
    };

    if let Some(t) = target {
        let max = (parent.scroll_height() as f64 - parent_h).max(0.0);
        parent.set_scroll_top(t.clamp(0.0, max) as i32);
    }
}

/// Centre `el` within its scroll `parent`, unconditionally.
///
/// Used for the deliberate "take me to where I am" gesture (re-clicking the
/// active sidebar tab), where the reader has explicitly asked to be moved and
/// the gentler `reveal_in_scroll_parent` would do nothing if the row happened
/// to already be barely on screen.
pub fn center_in_scroll_parent(el: &web_sys::Element, parent: &web_sys::Element) {
    let parent_h = parent.client_height() as f64;
    if parent_h <= 0.0 {
        return;
    }
    let er = el.get_bounding_client_rect();
    let pr = parent.get_bounding_client_rect();
    let scroll_top = parent.scroll_top() as f64;
    let top = er.top() - pr.top() + scroll_top;
    let target = top - (parent_h - er.height()) / 2.0;
    let max = (parent.scroll_height() as f64 - parent_h).max(0.0);
    parent.set_scroll_top(target.clamp(0.0, max) as i32);
}
