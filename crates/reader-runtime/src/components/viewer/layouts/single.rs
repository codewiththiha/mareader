//! Single-page layout: one centered page host, remounted per page turn.

use app_chrome::hooks::dom::SINGLE_PAGE_CONTAINER_ID;
use leptos::prelude::*;

use crate::components::viewer::shells::page_shell::PageShell;
use crate::components::viewer::{PageSlot, UniversalPageHost};
use crate::state::ReaderState;

#[component]
pub fn SingleLayout(
    state: ReaderState,
    #[prop(into)] progress_visible: Signal<bool>,
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
