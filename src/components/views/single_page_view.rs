//! Single-page (page-at-a-time) view. OWNED BY branch B (viewer/chrome).
//! One centered PageCanvas in a scroll container. Container size is tracked via
//! ResizeObserver; fit modes (Width/Page) are recomputed reactively.
//!
//! `page` is remounted per page by a keyed `<For>` (the PageCanvas organism keys
//! on ids, not the page number — a fresh wrapper + host is mounted per page).
//! Each remount gets a direction-aware entrance animation (`page-enter-right`
//! for next page, `page-enter-left` for previous) driven by a non-reactive
//! `StoredValue` of the previously shown page.
//!
//! The `<For>` keying is load-bearing: a plain reactive block would patch the
//! wrapper div in place (tachys `value.rebuild`), so the CSS animation would not
//! restart on consecutive same-direction turns. Only a keyed remount inserts a
//! fresh node for the animation to replay on — and the unchanged key on a
//! same-page set leaves the existing node untouched (no spurious animation).

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::ResizeObserverEntry;

use crate::components::organisms::page_canvas::PageCanvas;
use crate::core::state::AppState;

#[component]
pub fn SinglePageView(state: AppState) -> impl IntoView {
    let display_scale = state.viewer.display_scale.read_only();

    // Previously shown page, for the direction-aware page-turn animation.
    // Non-reactive on purpose: reading it inside the render closure doesn't
    // subscribe, so the wrapper only re-renders on the reactive `page` signal.
    // 0 = no history (first render) → default to "right" (forward).
    let prev_page = StoredValue::new(0u32);

    // --- Container-size tracking (ResizeObserver) ---------------------------
    // The observer + callback live in local-storage StoredValues so the JS
    // references stay alive for the component's lifetime and are dropped when
    // the component's reactive owner is disposed (no manual cleanup needed).
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
            .and_then(|d| d.get_element_by_id("single-page-container"))
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

    // MUST disconnect before the Closure is dropped — see the identical note in
    // continuous_view.rs. A mode switch unmounts this view and removes
    // `#single-page-container`, which queues a resize notification into a
    // closure that is about to be freed; without an explicit disconnect the
    // wasm runtime aborts with "closure invoked recursively or after being
    // dropped".
    on_cleanup(move || {
        if let Some(observer) = observer_handle.try_get_value().flatten() {
            observer.disconnect();
        }
        observer_handle.try_set_value(None);
        callback_handle.try_set_value(None);
    });

    // Fit-width/fit-page scale computation now lives in the app-root
    // `effects::fit::fit_effect` (shared with the continuous view).

    view! {
        <div
            id="single-page-container"
            class="flex h-full w-full items-start justify-center overflow-auto bg-surface"
        >
            <div class="p-6">
                <For
                    // Single-item keyed list: the key IS the page, so a page
                    // change unmounts the old wrapper and mounts a fresh one
                    // (replaying the entrance animation), while a same-page
                    // set leaves the mounted node untouched.
                    each=move || std::iter::once(state.viewer.page.get())
                    key=|p: &u32| *p
                    children=move |page: u32| {
                        // Direction: "right" when moving forward (or first
                        // render), "left" when going back. Record this page as
                        // `prev` BEFORE the view so the next render sees the
                        // new history. Static literal classes only (repo rule).
                        let prev = prev_page.get_value();
                        let dir = if prev == 0 || page > prev {
                            "page-enter-right"
                        } else {
                            "page-enter-left"
                        };
                        prev_page.set_value(page);
                        view! {
                            <div class=dir>
                                <PageCanvas
                                    page=page
                                    scale=display_scale
                                    canvas_id=format!("sp-{page}-cv")
                                    host_id=format!("sp-{page}-pg")
                                    render_text=true
                                />
                            </div>
                        }
                    }
                />
            </div>
        </div>
    }
}
