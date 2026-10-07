//! Scroll-vertical layout: continuous reading, from the page host.

use leptos::prelude::*;
use virtual_list_leptos::Virtualizer;

use crate::components::viewer::UniversalStreamHost;
use crate::state::ReaderState;

#[component]
pub fn ScrollVerticalLayout(
    state: ReaderState,
    virtualizer: Virtualizer,
    #[prop(into)] progress_visible: Signal<bool>,
) -> impl IntoView {
    view! {
        <UniversalStreamHost
            state=state
            virtualizer=virtualizer
            progress_visible=progress_visible
        />
    }
}
