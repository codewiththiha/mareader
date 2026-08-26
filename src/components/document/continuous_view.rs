//! Continuous vertical-scroll view.

use leptos::prelude::*;
use virtual_list_leptos::Virtualizer;

use crate::components::primitives::floating::types::z::CONTROLS;
use crate::components::primitives::hooks::dom::PAGE_LIST_ID;
use crate::components::primitives::hooks::use_resize_observer::observe_content_size;
use crate::state::ReaderState;

#[component]
pub fn ContinuousView(
    state: ReaderState,
    /// The virtualizer driving the page window, built by ReaderPage and shared
    /// with navigation sync and the zoom coordinator.
    virtualizer: Virtualizer,
) -> impl IntoView {
    crate::effects::reader::continuous_scroll::continuous_scroll(state);
    observe_content_size(PAGE_LIST_ID, state.viewer.container_size);

    // Bridge for the not-yet-migrated consumers of `viewer.scroll_top`
    // (relayout_to, fit, AI anchors, the bottom-bar scrubber, ...): keep it
    // in step with the virtualizer's coalesced scroll. `continuous_scroll`
    // currently writes the same value from the raw DOM scroll event; when
    // it is audited, its write goes away and this bridge becomes the only
    // writer.
    {
        let scroll_top = state.viewer.scroll_top;
        let offset = virtualizer.scroll_offset();
        Effect::new(move |_| scroll_top.set(offset.get()));
    }

    let total_height = virtualizer.total_size();
    let scroll_offset = virtualizer.scroll_offset();
    let progress = move || {
        let st = scroll_offset.get();
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
            <crate::components::document::PageList state=state virtualizer=virtualizer />
            <div
                class=format!(
                    "pointer-events-none absolute inset-x-0 bottom-0 {CONTROLS} h-0.5"
                )
            >
                <div
                    class="h-full bg-accent/80 transition-[width] duration-100"
                    style:width=move || format!("{}%", progress() * 100.0)
                ></div>
            </div>
        </div>
    }
}
