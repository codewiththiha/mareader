//! Single-page (`single`) layout: one centered page host in a scroll container.
//! `page` is remounted per page by a keyed `<For>` so each page turn gets a
//! fresh host (and the engine re-registers the canvas id); page turns are
//! instant — the reader's motion principles forbid entrance animations on
//! document content.
//!
//! The layout is format-free by construction: it mounts
//! [`UniversalPageHost`](crate::components::viewer::page_host::UniversalPageHost),
//! which reads the format tracked, so opening a document of the other kind swaps
//! the page inside this same slot. Nothing here names a raster, type, or engine.

use leptos::prelude::*;
use app_chrome::hooks::dom::SINGLE_PAGE_CONTAINER_ID;

use crate::components::viewer::{PageSlot, UniversalPageHost};
use crate::components::viewer::shells::page_shell::PageShell;
use crate::state::ReaderState;

#[component]
pub fn SingleLayout(
    state: ReaderState,
    #[prop(into)]
    progress_visible: Signal<bool>,
) -> impl IntoView {
    view! {
        <PageShell
            state=state
            scroller_id=SINGLE_PAGE_CONTAINER_ID
            progress_visible=progress_visible
        >
            <For
                each=move || std::iter::once(state.viewer.page.get())
                key=|p: &u32| *p
                children=move |page: u32| view! {
                    <UniversalPageHost page=page state=state page_slot=PageSlot::Single class="mx-auto" />
                }
            />
        </PageShell>
    }
}
