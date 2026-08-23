//! The standard square icon button (h-9 w-9, hover-only background). The
//! titlebar pin, the toolbar "…" overflow trigger and the More-menu trigger
//! all used identical raw markup; this owns it once.

use leptos::prelude::*;

use crate::components::primitives::icon::{Icon, IconName};

#[component]
pub fn IconButton(
    icon: IconName,
    #[prop(into, optional)] title: Option<String>,
    on_click: impl Fn() + 'static,
    #[prop(default = 18)] size: u16,
    /// Optional toggle state: renders `aria-pressed` plus the accent/ink
    /// colour swap (the titlebar pin).
    #[prop(optional)] pressed: Option<Signal<bool>>,
) -> impl IntoView {
    let pressed_sig = pressed.unwrap_or_else(|| Signal::derive(|| false));
    view! {
        <button
            type="button"
            title=title
            aria-pressed=pressed.map(|_| move || pressed_sig.get().to_string())
            on:click=move |_| on_click()
            class="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-transparent bg-transparent transition-colors hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            class=("text-accent", move || pressed_sig.get())
            class=("text-ink", move || !pressed_sig.get())
        >
            <Icon name=icon size=size />
        </button>
    }
}
