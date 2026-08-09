//! Search results panel. OWNED BY branch C (panels/sidebar).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn SearchPanel(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch C): SearchBox + result list (page badge + snippet);
    // click -> jump to match (single: set page; continuous: scroll to rect).
    view! { <div class="flex flex-1 flex-col overflow-hidden" /> }
}
