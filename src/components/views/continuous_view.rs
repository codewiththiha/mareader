//! Continuous vertical-scroll view. OWNED BY branch A (viewer/continuous).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn ContinuousView(state: AppState) -> impl IntoView {
    // Runs once per mount: attaches the scroll listener on #page-list and
    // cleans it up when the view unmounts (mode switch / document close).
    crate::effects::continuous_scroll::continuous_scroll(state);

    view! {
        <crate::components::organisms::page_list::PageList state=state />
    }
}
