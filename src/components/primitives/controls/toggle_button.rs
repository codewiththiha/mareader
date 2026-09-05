//! Toggle button: the generic selected/unselected control for rail toggles,
//! option buttons and segmented-like controls. `aria-pressed` + the
//! filled/quiet state pair, with the caller supplying only the layout
//! (`variant_class`) and the content.

use leptos::prelude::*;

/// A pressed/quiet toggle: the shared pressed-state shell the rail toggles /
/// option buttons / segmented-like controls consolidate onto (the sidebar
/// rail is the first consumer).
#[component]
pub fn ToggleButton(
    active: Signal<bool>,
    on_click: impl Fn() + 'static,
    #[prop(into, optional)] title: Option<String>,
    /// Caller layout classes (size, padding, shape).
    #[prop(optional)]
    variant_class: &'static str,
    #[prop(default = false)]
    disabled: bool,
    children: Children,
) -> impl IntoView {
    let class = move || {
        let base = "inline-flex items-center justify-center rounded-lg transition-colors \
                    focus:outline-none focus-visible:ring-2 focus-visible:ring-accent \
                    disabled:pointer-events-none disabled:opacity-50";
        if active.get() {
            format!("{base} {variant_class} bg-line text-ink font-medium")
        } else {
            format!("{base} {variant_class} text-muted hover:text-ink")
        }
    };

    view! {
        <button
            type="button"
            title=title.clone()
            aria-label=title
            aria-pressed=move || active.get().to_string()
            disabled=disabled
            on:click=move |_| on_click()
            class=class
        >
            {children()}
        </button>
    }
}
