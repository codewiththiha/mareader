//! Small uppercase section header used by menus.

use leptos::prelude::*;

#[component]
pub fn SectionLabel(#[prop(into)] text: String) -> impl IntoView {
    view! {
        <p class="mb-1 text-[10px] font-semibold uppercase tracking-wide text-muted">
            {text}
        </p>
    }
}
