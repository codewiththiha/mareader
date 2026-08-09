//! Reusable button atom.

use leptos::ev::MouseEvent;
use leptos::prelude::*;

use super::icon::IconName;
use crate::components::atoms::icon::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// Toolbar-style bordered button.
    Toolbar,
    /// Icon button with no border (hover-only background).
    Ghost,
    /// Solid accent primary button.
    Primary,
}

#[component]
pub fn Button(
    on_click: impl Fn(MouseEvent) + 'static,
    #[prop(optional)] kind: Option<ButtonKind>,
    #[prop(optional)] icon: Option<IconName>,
    #[prop(optional)] label: Option<String>,
    #[prop(optional)] title: Option<String>,
    #[prop(default = false)] active: bool,
    #[prop(default = false)] disabled: bool,
    #[prop(optional)]
    children: Option<Children>,
) -> impl IntoView {
    let base = "inline-flex items-center justify-center gap-1.5 rounded-lg border h-9 px-2.5 text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:pointer-events-none disabled:opacity-50 whitespace-nowrap";
    let kind_class = match kind.unwrap_or(ButtonKind::Ghost) {
        ButtonKind::Toolbar => "border-line bg-surface text-ink hover:bg-line",
        ButtonKind::Ghost => "border-transparent bg-transparent text-ink hover:bg-line",
        ButtonKind::Primary => "border-transparent bg-accent text-white hover:brightness-110",
    };
    let state_class = if active {
        "border-accent text-accent"
    } else {
        ""
    };

    view! {
        <button
            type="button"
            on:click=move |ev| on_click(ev)
            title=title
            disabled=disabled
            class=(base.to_string() + " " + kind_class + " " + state_class)
        >
            {icon.map(|name| view! { <Icon name=name size=16/> })}
            {label.map(|l| view! { <span>{l}</span> })}
            {children.map(|c| c())}
        </button>
    }
}
