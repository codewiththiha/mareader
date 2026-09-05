//! Thin divider.

use leptos::prelude::*;

#[component]
pub fn Separator(#[prop(default = false)] vertical: bool) -> impl IntoView {
    if vertical {
        view! { <div class="mx-1 h-6 w-px shrink-0 bg-line" /> }
    } else {
        view! { <div class="h-px w-full shrink-0 bg-line" /> }
    }
}
