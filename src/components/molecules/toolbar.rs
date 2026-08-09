//! Top toolbar. OWNED BY branch B (viewer/chrome).
//! Layout: left = open + view-mode toggle; center = page nav; right = zoom +
//! theme/texture/noise menus + sidebar toggles.

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn Toolbar(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch B): full toolbar with Open button, view mode switch,
    // ZoomControls, ThemeMenu, TextureMenu, NoiseToggle, sidebar toggles.
    view! { <div class="flex h-12 items-center gap-2 border-b border-line bg-surface px-3" /> }
}
