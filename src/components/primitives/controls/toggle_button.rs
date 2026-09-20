//! Toggle button: the pressed/quiet control every selected-or-not button in
//! the app consolidates onto. The component owns `aria-pressed` and the state
//! classes; the caller owns only the layout (`variant_class`) and the content.
//!
//! The app has two shapes of "this is the one" — the bordered accent chip the
//! option groups use, and the borderless filled chip the sidebar rail uses.
//! [`ToggleVariant`] picks between them, so neither needs a second component.

use leptos::prelude::*;

/// Which of the two pressed-state treatments the button wears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleVariant {
    /// Bordered chip that tints to the accent when selected: the option
    /// groups — base mode, texture, grain, import formats, sort direction.
    Outlined,
    /// Borderless chip that fills when active: the sidebar rail's toggles.
    Filled,
}

#[component]
pub fn ToggleButton(
    active: Signal<bool>,
    on_click: impl Fn() + 'static,
    #[prop(into, optional)] title: Option<String>,
    /// Caller layout classes (size, padding, shape). The state classes belong
    /// to this component, so every call site renders the same pressed state.
    #[prop(optional)]
    variant_class: &'static str,
    #[prop(default = ToggleVariant::Outlined)]
    variant: ToggleVariant,
    children: Children,
) -> impl IntoView {
    let class = move || match variant {
        ToggleVariant::Outlined => {
            if active.get() {
                format!("rounded-md border border-accent bg-accent-soft font-medium {variant_class} text-accent")
            } else {
                format!("rounded-md border border-line {variant_class} text-ink hover:bg-line")
            }
        }
        ToggleVariant::Filled => {
            let base = "inline-flex items-center justify-center rounded-lg transition-colors \
                        focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";
            if active.get() {
                format!("{base} {variant_class} bg-line text-ink font-medium")
            } else {
                format!("{base} {variant_class} text-muted hover:text-ink")
            }
        }
    };

    view! {
        <button
            type="button"
            title=title.clone()
            aria-label=title
            aria-pressed=move || active.get().to_string()
            on:click=move |_| on_click()
            class=class
        >
            {children()}
        </button>
    }
}
