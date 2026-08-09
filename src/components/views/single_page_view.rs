//! Single-page (page-at-a-time) view. OWNED BY branch B (viewer/chrome).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn SinglePageView(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch B): one centered PageCanvas in a scroll container; fit-width /
    // fit-page via math::fit_scale; prev/next from viewer.page.
    view! { <div class="flex h-full w-full items-center justify-center overflow-auto bg-surface" /> }
}
