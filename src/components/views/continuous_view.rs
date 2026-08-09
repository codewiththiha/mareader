//! Continuous vertical-scroll view. OWNED BY branch A (viewer/continuous).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn ContinuousView(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch A): PageList + scroll effect (effects::continuous_scroll).
    view! { <crate::components::organisms::page_list::PageList state=state /> }
}
