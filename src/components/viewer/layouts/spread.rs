//! Spread (`spread`) layout: two pages side by side with no gap.
//!
//! Both halves go through the page host with a slot that says which side of the
//! spine they are on, and that is the whole of this file's contribution to
//! pagination: under a book layout each host carries its own gutter-side padding,
//! so the spine falls exactly where the two hosts meet — for a document of rasters
//! and a document of type alike.

use leptos::prelude::*;
use app_chrome::hooks::dom::DUAL_PAGE_CONTAINER_ID;

use crate::components::viewer::{PageSlot, UniversalPageHost};
use crate::components::viewer::shells::page_shell::PageShell;
use crate::state::ReaderState;

#[component]
pub fn SpreadLayout(
    state: ReaderState,
    #[prop(into)]
    progress_visible: Signal<bool>,
) -> impl IntoView {
    view! {
        <PageShell
            state=state
            scroller_id=DUAL_PAGE_CONTAINER_ID
            progress_visible=progress_visible
        >
            <For
                each=move || std::iter::once(reader_core::view::spread_index(state.viewer.page.get()))
                key=|s: &u32| *s
                children=move |spread: u32| {
                    let p1 = spread * 2 + 1;
                    let p2 = p1 + 1;
                    view! {
                        <div class="flex items-start justify-center gap-0">
                            <UniversalPageHost page=p1 state=state page_slot=PageSlot::SpreadLeft />
                            {move || {
                                let n = state.document.num_pages.get();
                                if p2 <= n {
                                    view! {
                                        <UniversalPageHost
                                            page=p2
                                            state=state
                                            page_slot=PageSlot::SpreadRight
                                        />
                                    }
                                    .into_any()
                                } else {
                                    ().into_any()
                                }
                            }}
                        </div>
                    }
                }
            />
        </PageShell>
    }
}
