//! A choice row: a name, and the line under it that says what choosing it
//! does.
//!
//! Not [`MenuItem`](super::menu_item::MenuItem), which is a command — an
//! icon, a label, one line tall. This is an answer to a question a sheet is
//! asking, and the difference is the note: a merge, a replace and an "as
//! new" are consequences the reader must be able to read before the click,
//! so the note wraps and no icon column competes with it.
//!
//! Lives here rather than in the collision sheet that first needed it: two
//! sheets importing a row shape from a third file in one feature is a
//! primitive with the wrong address.

use leptos::prelude::*;

/// One answer on a sheet: its name, and the line that promises what it does.
#[component]
pub fn ChoiceRow(
    /// The answer's own name — "Merge", "Add as new", "Replace".
    label: &'static str,
    /// What choosing it does, in the sheet's own words. Wraps: a promise
    /// the reader has to take on faith is not a promise.
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
