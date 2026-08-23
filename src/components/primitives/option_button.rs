//! Option button: the selected/unselected option pattern shared by the
//! appearance sections (base mode, texture mode, film grain).
//!
//! All three sections repeated the same `aria-pressed` + selected-class
//! contract (`border-accent bg-accent-soft font-medium text-accent` vs
//! `border-line text-ink hover:bg-line`); this owns it once. `variant_class`
//! carries each section's own layout/padding/size classes; the state classes
//! stay identical everywhere.

use leptos::prelude::*;

#[component]
pub fn OptionButton(
    selected: Signal<bool>,
    on_click: impl Fn() + 'static,
    #[prop(into, optional)]
    title: Option<String>,
    /// Section-specific layout classes (flex/padding/size). The selected /
    /// unselected state classes are owned here and must stay identical.
    #[prop(optional)]
    variant_class: &'static str,
    children: Children,
) -> impl IntoView {
    view! {
        <button
            type="button"
            title=title
            aria-pressed=move || selected.get().to_string()
            on:click=move |_| on_click()
            class=move || {
                if selected.get() {
                    format!(
                        "rounded-md border border-accent bg-accent-soft font-medium {variant_class} text-accent"
                    )
                } else {
                    format!("rounded-md border border-line {variant_class} text-ink hover:bg-line")
                }
            }
        >
            {children()}
        </button>
    }
}
