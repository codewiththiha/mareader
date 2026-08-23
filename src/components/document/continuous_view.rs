//! Continuous vertical-scroll view.

use leptos::prelude::*;

use crate::state::ReaderState;
use pdf_core::layout::DocumentLayout;
use crate::components::document::dom_helpers::{observe_content_size, PAGE_LIST_ID};

#[component]
pub fn ContinuousView(
    state: ReaderState,
    /// The cached column layout, built once by the reader page (shared with
    /// PageList, the zoom anchor and page tracking).
    layout: Memo<DocumentLayout>,
) -> impl IntoView {
    // Runs once per mount: attaches the scroll listener on #page-list and
    // cleans it up when the view unmounts (mode switch / document close).
    crate::effects::reader::continuous_scroll::continuous_scroll(state);

    // Container-size tracking: reports the #page-list content box into
    // viewer.container_size so fit modes and the visible-page window use the
    // real dimensions.
    observe_content_size(PAGE_LIST_ID, state.viewer.container_size);

    // Reading-progress bar: fraction of the scrollable extent consumed.
    let total_height = Memo::new(move |_| layout.with(|l| l.total()));
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
            <crate::components::document::PageList state=state layout=layout />
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
