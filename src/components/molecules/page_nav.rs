//! Previous/next + page-number input. OWNED BY branch B (viewer/chrome).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn PageNav(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch B): Prev/Next buttons + "current / total" input.
    view! { <div class="flex items-center gap-1" /> }
}
