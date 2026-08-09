//! Bottom status bar (page x/y, zoom %, doc title/error). OWNED BY branch B
//! (viewer/chrome).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn StatusBar(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch B): current page / total, scale %, document title or error.
    view! { <div class="flex h-8 items-center gap-3 border-t border-line bg-surface px-3 text-xs text-muted" /> }
}
