//! Reusable button container: the variant owns the styling, the children own
//! the content (`<Icon .../><span>"Open"</span>`). Icon-only buttons use
//! [`IconButton`](super::icon_button::IconButton).

use leptos::ev::MouseEvent;
use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// Toolbar-style bordered button.
    Toolbar,
    /// No border (hover-only background).
    #[default]
    Ghost,
    /// Solid accent primary button.
    Primary,
}

#[component]
pub fn Button(
    on_click: impl Fn(MouseEvent) + 'static,
    children: Children,
    #[prop(default = ButtonVariant::Ghost)]
    variant: ButtonVariant,
    #[prop(into, optional)] title: Option<String>,
    #[prop(default = false)] active: bool,
    #[prop(default = false)] disabled: bool,
) -> impl IntoView {
    let base = "inline-flex items-center justify-center gap-1.5 rounded-lg border h-9 px-2.5 text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:pointer-events-none disabled:opacity-50 whitespace-nowrap";
    let variant_class = match variant {
        ButtonVariant::Toolbar => "border-line bg-surface text-ink hover:bg-line",
        ButtonVariant::Ghost => "border-transparent bg-transparent text-ink hover:bg-line",
        ButtonVariant::Primary => "border-transparent bg-accent text-white hover:brightness-110",
    };
    let state_class = if active {
        "border-accent text-accent"
    } else {
        ""
    };

    view! {
        <button
            type="button"
            on:click=on_click
            title=title
            disabled=disabled
            class=base.to_string() + " " + variant_class + " " + state_class
        >
            {children()}
        </button>
    }
}
