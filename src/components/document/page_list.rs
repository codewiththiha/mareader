//! Virtualized scroll container for the continuous layout.
//!
//! A scroll container (`#page-list`) holding:
//!  - an in-flow spacer whose height equals the full document height, so the
//!    scrollbar spans the whole column,
//!  - a keyed `<For>` over the mounted page window, driven by the shared
//!    virtualizer: visible pages plus a screenful-based read-ahead, unioned
//!    with the pinned page range.
//!
//! Each page is an absolutely-positioned wrapper centered in the column,
//! containing a shared foundation `PageCanvas`. Evicted pages are unmounted by
//! `<For>`; entering pages render via `PageCanvas`'s own scale effect.
//!
//! Heights flow two ways:
//!  - estimates: each page is seeded from its own intrinsic size at the current
//!    scale,
//!  - measurements: `on_geometry` reports the rendered height into
//!    `css_heights` AND into the virtualizer, which applies it in its next
//!    batched flush with scroll anchoring.

use leptos::html;
use leptos::prelude::*;
use virtual_list_leptos::{Align, ScrollMode, VirtualItem, Virtualizer};
use wasm_bindgen::JsCast;

use crate::components::document::PageCanvas;
use crate::components::document::page_canvas::component::GlossOverlayProps;
use crate::state::{ReaderState, TextureSignal};

#[component]
pub fn PageList(
    state: ReaderState,
    /// The virtualizer driving the window, spacer, and scroll anchoring.
    virtualizer: Virtualizer,
) -> impl IntoView {
    let texture =
        use_context::<TextureSignal>().expect("TextureSignal must be provided by app bootstrap");

    let v = virtualizer;
    let list_ref: NodeRef<html::Div> = NodeRef::new();
    {
        let v = v.clone();
        Effect::new(move |_| {
            let Some(div) = list_ref.get() else {
                return;
            };
            let el: web_sys::Element = div.clone().unchecked_into();
            v.bind_container(el);
            // Re-seed the viewport + window so `items()` is non-empty right
            // away. The virtualizer is created once in ReaderPage and its
            // container binding goes stale whenever this view unmounts (a
            // single → continuous switch): without re-measuring, the window
            // would not re-seed and nothing would render.
            v.remeasure_container();

            let page = state.viewer.page.get_untracked();
            if page > 0 {
                v.scroll_to_index((page - 1) as usize, Align::Start, ScrollMode::Instant);
            }
        });
    }

    let display_scale = state.viewer.zoom.display.read_only();
    let virtualizer_handle = StoredValue::new_local(v.clone());
    let on_geometry = Callback::new(move |(page, _width, height): (u32, f64, f64)| {
        if state.viewer.zoom_animating.get_untracked() {
            return;
        }
        let index = page.saturating_sub(1) as usize;
        state.document.metrics.css_heights.update(|heights| {
            while heights.len() <= index {
                heights.push(0.0);
            }
            heights[index] = height;
        });
        virtualizer_handle.with_value(|virtualizer| virtualizer.report_size(index, height));
    });

    let items = v.items();
    let total_size = v.total_size();

    view! {
        <div
            id="page-list"
            node_ref=list_ref
            class="h-full w-full overflow-y-auto outline-none"
            tabindex="0"
        >
            // Inner column, offset by the toolbar height so the first page
            // starts below the glass header while the scrollport itself still
            // runs the full height of the window.
            <div class="relative mt-12">
                <div
                    aria-hidden="true"
                    style:height=move || format!("{}px", total_size.get())
                ></div>
                <For
                    each=move || items.get()
                    key=|item: &VirtualItem| item.index
                    children=move |item: VirtualItem| {
                        let index = item.index;
                        let top = virtualizer_handle.with_value(|virtualizer| virtualizer.item_top(index));
                        let style = move || {
                            format!(
                                "position:absolute;top:{}px;left:0;right:0;display:flex;justify-content:center",
                                top.get()
                            )
                        };
                        view! {
                            <div id=format!("cont-{index}-wrap") style=style>
                                <PageCanvas
                                    page={(index + 1) as u32}
                                    scale=display_scale
                                    render_scale=state.viewer.zoom.render
                                    zoom_animating=state.viewer.zoom_animating
                                    texture=texture
                                    canvas_id=format!("cont-{index}-cv")
                                    host_id=format!("cont-{index}-pg")
                                    render_text=true
                                    on_geometry=on_geometry
                                    gloss_overlay=GlossOverlayProps::from_gloss(state.gloss)
                                />
                            </div>
                        }
                    }
                />
            </div>
        </div>
    }
}
