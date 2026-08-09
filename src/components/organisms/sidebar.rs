//! Left sidebar with tabs (outline / search / thumbnails). OWNED BY branch C
//! (panels/sidebar).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn Sidebar(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch C): rail of SidebarItems + active panel (OutlinePanel /
    // SearchPanel / ThumbnailsPanel). Renders a stub panel until branch D lands.
    view! { <div class="flex w-72 shrink-0 flex-col border-r border-line bg-surface" /> }
}
