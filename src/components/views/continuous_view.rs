//! Continuous vertical-scroll view. OWNED BY branch A (viewer/continuous).

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::ResizeObserverEntry;

use crate::core::state::AppState;

#[component]
pub fn ContinuousView(state: AppState) -> impl IntoView {
    // Runs once per mount: attaches the scroll listener on #page-list and
    // cleans it up when the view unmounts (mode switch / document close).
    crate::effects::continuous_scroll::continuous_scroll(state);

    // --- Container-size tracking (ResizeObserver) ---------------------------
    // Mirrors SinglePageView: reports the #page-list content size into
    // viewer.container_size so fit modes and the visible-page window use the
    // real dimensions. Observer + callback live in local StoredValues so the JS
    // references stay alive for the view's lifetime and drop on unmount.
    let observer_handle = StoredValue::new_local(None::<web_sys::ResizeObserver>);
    let callback_handle =
        StoredValue::new_local(None::<Closure<dyn FnMut(Vec<ResizeObserverEntry>)>>);

    Effect::new(move || {
        // Guard: only set up once (StoredValue access is non-reactive).
        if callback_handle.with_value(|c| c.is_some()) {
            return;
        }
        let Some(el) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("page-list"))
        else {
            return;
        };
        let st = state;
        let callback: Closure<dyn FnMut(Vec<ResizeObserverEntry>)> = Closure::wrap(
            Box::new(move |entries: Vec<ResizeObserverEntry>| {
                if let Some(entry) = entries.first() {
                    let rect = entry.content_rect();
                    st.viewer.container_size.set((rect.width(), rect.height()));
                }
            }) as Box<dyn FnMut(Vec<ResizeObserverEntry>)>,
        );
        let fn_ref: &js_sys::Function = callback.as_ref().unchecked_ref();
        if let Ok(observer) = web_sys::ResizeObserver::new(fn_ref) {
            observer.observe(&el);
            observer_handle.set_value(Some(observer));
            callback_handle.set_value(Some(callback));
        }
    });

    // MUST disconnect before the Closure is dropped. Dropping the Rust handles
    // does NOT stop the browser-side observer: it keeps a reference to the
    // wasm-bindgen shim, and unmounting this view (a mode switch) removes
    // `#page-list`, which itself queues a resize notification. That callback
    // then reaches a freed closure and the runtime aborts with "closure invoked
    // recursively or after being dropped". Disconnecting first guarantees no
    // further callbacks can be delivered.
    on_cleanup(move || {
        if let Some(observer) = observer_handle.try_get_value().flatten() {
            observer.disconnect();
        }
        observer_handle.try_set_value(None);
        callback_handle.try_set_value(None);
    });

    // "How far through the document am I" indicator (U8): fraction of the
    // scrollable range currently passed. The total scrollable height is
    // memoized over `page_heights` so scroll ticks only re-read the numerator
    // (heights only change on render/zoom, not on scroll).
    let total_height = Memo::new(move |_| {
        let heights = state.doc.page_heights.get();
        crate::core::layout::total_height_css(&heights, crate::core::layout::PAGE_GAP)
    });
    let progress = move || {
        let st = state.viewer.scroll_top.get();
        let (_, vh) = state.viewer.container_size.get();
        let total = total_height.get();
        if total > vh && total > 0.0 {
            (st / (total - vh)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };

    view! {
        <div class="relative h-full w-full">
            <crate::components::organisms::page_list::PageList state=state />
            // Thin scroll-progress bar pinned to the bottom of the view. The
            // outer track is pointer-events-none so it never blocks scrolling.
            <div class="pointer-events-none absolute inset-x-0 bottom-0 z-30 h-0.5">
                <div
                    class="h-full bg-accent/80 transition-[width] duration-100"
                    style:width=move || format!("{}%", progress() * 100.0)
                ></div>
            </div>
        </div>
    }
}
