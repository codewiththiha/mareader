//! A labelled row: a name left, whatever answers it right.

use leptos::prelude::*;

/// One labelled row.
#[component]
pub fn Row(
    /// The name on the left; a static string, not reader input.
    label: &'static str,
    /// The control, value or switch that answers it.
    children: Children,
) -> impl IntoView {
    view! {
        <div class="flex items-center justify-between gap-3 px-4 py-3.5">
            <span class="text-sm text-ink">{label}</span>
            {children()}
        </div>
    }
}
