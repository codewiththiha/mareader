//! A choice row: a name and the note saying what choosing it does.

use leptos::prelude::*;

/// One answer on a sheet: its name, and the line that promises what it does.
#[component]
pub fn ChoiceRow(
    /// The answer's own name — "Merge", "Add as new", "Replace".
    label: &'static str,
    /// What choosing it does, in the sheet's own words; wraps.
    #[prop(into)]
    note: String,
    on_click: Callback<()>,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class="flex w-full flex-col gap-0.5 px-3.5 py-2.5 text-left transition-colors \
                   hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            on:click=move |_| on_click.run(())
        >
            <span class="text-sm text-ink">{label}</span>
            <span class="text-xs text-muted">{note}</span>
        </button>
    }
}
