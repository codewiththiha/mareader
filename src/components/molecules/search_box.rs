//! Search query input + submit. OWNED BY branch C (panels/sidebar).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn SearchBox(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch C): input bound to search.query + submit that runs the search.
    view! { <div class="flex items-center gap-1" /> }
}
