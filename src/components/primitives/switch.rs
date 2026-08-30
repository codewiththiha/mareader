//! Toggle switch control.

use leptos::prelude::*;

#[component]
pub fn Switch(
    checked: Signal<bool>,
    on_change: Callback<bool>,
    #[prop(into, optional)] title: Option<String>,
    #[prop(into, default = Signal::derive(|| false))] disabled: Signal<bool>,
) -> impl IntoView {
    let class = move || {
        let base = "relative inline-flex h-6 w-11 shrink-0 items-center rounded-full border transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:cursor-not-allowed disabled:opacity-45";
        if checked.get() {
            format!("{base} border-transparent bg-accent/80")
        } else {
            format!("{base} border-line bg-line")
        }
    };
    let knob = move || {
        let base = "inline-block h-4 w-4 transform rounded-full bg-white/90 shadow transition-transform";
        if checked.get() {
            format!("{base} translate-x-6")
        } else {
            format!("{base} translate-x-1")
        }
    };
    view! {
        <button
            type="button"
            role="switch"
            aria-checked=move || checked.get().to_string()
            title=title.clone()
            aria-label=title
            prop:disabled=move || disabled.get()
            on:click=move |_| on_change.run(!checked.get_untracked())
            class=class
        >
            <span class=knob></span>
        </button>
    }
}
