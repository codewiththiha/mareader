//! Virtualized scroll container for the continuous layout. OWNED BY branch A
//! (viewer/continuous).
//!
//! A scroll container (`#page-list`) holding:
//!  - an in-flow spacer whose height equals the full document height, so the
//!    scrollbar spans the whole column,
//!  - a keyed `<For>` over the visible page window (plus a `SCROLL_BUFFER`
//!    margin). Each page is an absolutely-positioned wrapper centered in the
//!    column, containing a shared foundation `PageCanvas`. Evicted pages are
//!    unmounted by `<For>` (their `on_cleanup` unregisters them with the
//!    engine); entering pages render via PageCanvas's own `scale` effect.

use leptos::prelude::*;

use crate::components::organisms::page_canvas::PageCanvas;
use crate::core::layout::{
    page_top_css, total_height_css, visible_range, PAGE_GAP, SCROLL_BUFFER,
};
use crate::core::state::AppState;

#[component]
pub fn PageList(state: AppState) -> impl IntoView {
    // Seed unmeasured pages with a `page1_size.height * scale` placeholder so
    // the scrollbar spans the whole document before any page has rendered.
    // Only fires while the heights vector is empty; `on_geometry` reports from
    // PageCanvas then overwrite each entry with the real rendered height.
    Effect::new(move || {
        let n = state.doc.num_pages.get();
        let scale = state.viewer.render_scale.get();
        let base_h = state.doc.page1_size.get().map(|s| s.height).unwrap_or(0.0);
        if n > 0 && base_h > 0.0 {
            state.doc.page_heights.update(|v| {
                if v.is_empty() {
                    v.resize(n as usize, base_h * scale);
                }
            });
        }
    });

    // Visible page window [first, last] (0-based, inclusive), expanded by
    // SCROLL_BUFFER on each side. `None` => no pages to render yet.
    let visible = Memo::new(move |_| {
        let heights = state.doc.page_heights.get();
        visible_range(
            state.viewer.scroll_top.get(),
            state.viewer.container_size.get().1,
            &heights,
            PAGE_GAP,
            SCROLL_BUFFER,
        )
    });

    // The scale the layout is DRAWN at. During a zoom this moves every frame
    // and the canvases CSS-stretch to follow it; `render_scale` (what the
    // bitmaps were rasterised at) is read by PageCanvas itself and only changes
    // once, when the gesture settles.
    let display_scale = state.viewer.display_scale.read_only();

    // Store the real rendered height back into page_heights (0-based index).
    let on_geometry = Callback::new(move |(p, _w, h): (u32, f64, f64)| {
        // While a zoom animation is running the coordinator owns page_heights:
        // it rescales the whole vector per frame. A render that resolves
        // mid-flight would write ONE page's height at the old scale into that
        // vector, shifting every page below it and yanking the scroll — the
        // teleport bug in miniature. The post-settle render reports the true
        // height a moment later, so nothing is lost by skipping here.
        if state.viewer.zoom_animating.get_untracked() {
            return;
        }
        let idx = p.saturating_sub(1) as usize;
        state.doc.page_heights.update(|v| {
            while v.len() <= idx {
                v.push(0.0);
            }
            v[idx] = h;
        });
    });

    view! {
        <div id="page-list" class="relative h-full w-full overflow-y-auto">
            // Spacer: makes the scrollbar span the whole column.
            <div
                aria-hidden="true"
                style:height=move || {
                    let heights = state.doc.page_heights.get();
                    format!("{}px", total_height_css(&heights, PAGE_GAP))
                }
            ></div>
            <For
                each=move || {
                    visible
                        .get()
                        .map(|(first, last)| (first..=last).collect::<Vec<usize>>())
                        .unwrap_or_default()
                }
                key=|i: &usize| *i
                children=move |i: usize| {
                    let style = move || {
                        let heights = state.doc.page_heights.get();
                        let top = page_top_css(i, &heights, PAGE_GAP);
                        format!(
                            "position:absolute;top:{top}px;left:0;right:0;display:flex;justify-content:center"
                        )
                    };
                    view! {
                        <div id=format!("cont-{i}-wrap") style=style>
                            <PageCanvas
                                page={(i + 1) as u32}
                                scale=display_scale
                                canvas_id=format!("cont-{i}-cv")
                                host_id=format!("cont-{i}-pg")
                                render_text=true
                                on_geometry=on_geometry
                            />
                        </div>
                    }
                }
            />
        </div>
    }
}
