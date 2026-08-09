//! Single-page (page-at-a-time) view. OWNED BY branch B (viewer/chrome).
//! One centered PageCanvas in a scroll container. Container size is tracked via
//! ResizeObserver; fit modes (Width/Page) are recomputed reactively.
//!
//! `page` is remounted per page by rebuilding the PageCanvas with fresh
//! canvas/host ids (the PageCanvas organism keys on ids, not the page number).

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::ResizeObserverEntry;

use crate::components::organisms::page_canvas::PageCanvas;
use crate::core::math::{fit_scale, FitMode};
use crate::core::state::AppState;

#[component]
pub fn SinglePageView(state: AppState) -> impl IntoView {
    let render_scale = state.viewer.render_scale.read_only();

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

    // --- Fit effect ---------------------------------------------------------
    // Recomputes scale/render_scale whenever fit, container size, or page 1 size
    // change. `scale` is read untracked so writing it back doesn't self-trigger.
    Effect::new(move || {
        let fit = state.viewer.fit.get();
        if fit == FitMode::None {
            return;
        }
        let (cw, ch) = state.viewer.container_size.get();
        if let Some(p) = state.doc.page1_size.get() {
            let s = fit_scale(
                fit,
                cw,
                ch,
                p.width,
                p.height,
                48.0,
                state.viewer.scale.get_untracked(),
            );
            state.viewer.scale.set(s);
            state.viewer.render_scale.set(s);
        }
    });

    view! {
        <div
            id="single-page-container"
            class="flex h-full w-full items-start justify-center overflow-auto bg-surface"
        >
            <div class="p-6">
                {move || {
                    let p = state.viewer.page.get();
                    view! {
                        <PageCanvas
                            page=p
                            scale=render_scale
                            canvas_id=format!("sp-{p}-cv")
                            host_id=format!("sp-{p}-pg")
                            render_text=true
                        />
                    }
                }}
            </div>
        </div>
    }
}
