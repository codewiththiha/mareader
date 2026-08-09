//! Zoom controls. OWNED BY branch B (viewer/chrome).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn ZoomControls(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch B): zoom in/out, fit width, fit page, custom % input.
    view! { <div class="flex items-center gap-1" /> }
}
