//! Scroll-vertical layout: all pages in one vertical strip.

use leptos::prelude::*;
use pdf_core::layout::Axis;
use virtual_list_leptos::Virtualizer;

use crate::components::document::shells::scroll_shell::ScrollShell;
use crate::state::ReaderState;

#[component]
pub fn ScrollVerticalLayout(
    state: ReaderState,
    virtualizer: Virtualizer,
    #[prop(into)]
    progress_visible: Signal<bool>,
) -> impl IntoView {
    view! {
        <ScrollShell
            state=state
            virtualizer=virtualizer
            axis=Axis::Vertical
            progress_visible=progress_visible
        />
    }
}
