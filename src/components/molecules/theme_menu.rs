//! Eye-protection theme menu (light/dark/sepia/green/night/dim). OWNED BY branch D
//! (panels/settings).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn ThemeMenu(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch D): popover listing THEMES; click sets settings.theme_id.
    view! { <div class="inline-flex" /> }
}
