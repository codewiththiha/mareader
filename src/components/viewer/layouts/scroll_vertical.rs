//! Scroll-vertical layout: continuous reading.
//!
//! One line of substance: the reading surface comes from the page host, because
//! this is the one mode where the two pipelines disagree about the SURFACE itself
//! — a reflowable document reads as one uninterrupted column of blocks on the
//! window's own paper, a PDF as a strip of page hosts. Both mount under the same
//! scroller id, so everything that addresses the column by name (the overlay
//! scrollbar, the container observer, auto-scroll, the keyboard) serves either
//! unchanged.

use leptos::prelude::*;
use virtual_list_leptos::Virtualizer;

use crate::components::viewer::UniversalStreamHost;
use crate::state::ReaderState;

#[component]
pub fn ScrollVerticalLayout(
    state: ReaderState,
    virtualizer: Virtualizer,
    #[prop(into)]
    progress_visible: Signal<bool>,
) -> impl IntoView {
    view! {
        <UniversalStreamHost
            state=state
            virtualizer=virtualizer
            progress_visible=progress_visible
        />
    }
}
