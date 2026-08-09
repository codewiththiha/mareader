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

    view! {
        <crate::components::organisms::page_list::PageList state=state />
    }
}
