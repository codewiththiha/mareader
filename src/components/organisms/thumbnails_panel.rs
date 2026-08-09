//! Thumbnail grid. OWNED BY branch D (panels/settings).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn ThumbnailsPanel(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch D): grid of low-scale canvases (renderText=false) rendered on
    // panel open, cleaned up on close; click -> jump to page.
    view! { <div class="flex-1 overflow-y-auto" /> }
}
