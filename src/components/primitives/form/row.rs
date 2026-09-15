//! A labelled row: a name on the left, whatever answers it on the right.
//!
//! The smallest unit a settings panel or sheet is made of, and a primitive
//! rather than a `<div>` each caller spells: the gap, padding and label
//! colour are one look. Lives here rather than in the settings feature
//! because the library's import sheet and removal receipt are built from the
//! same rows.

use leptos::prelude::*;

/// One labelled row.
#[component]
pub fn Row(
    /// The name on the left. `&'static str` because a row's label is a
    /// sentence the code knows, not a value the reader typed.
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
