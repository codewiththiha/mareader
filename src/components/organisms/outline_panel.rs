//! Document outline (TOC) panel. OWNED BY branch C (panels/sidebar).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn OutlinePanel(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch C): render doc.outline tree, indent by depth, click -> page.
    view! { <div class="flex-1 overflow-y-auto" /> }
}
