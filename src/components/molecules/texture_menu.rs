//! Paper texture menu (none/paper/lined/grid/dotted/cross). OWNED BY branch D
//! (panels/settings).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn TextureMenu(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch D): popover listing TextureMode; click sets settings.texture.
    view! { <div class="inline-flex" /> }
}
