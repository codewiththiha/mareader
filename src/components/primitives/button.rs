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

/// Semantic tone overrides the neutral text/border colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonTone {
    #[default]
    Neutral,
    /// Destructive action styling.
    Danger,
}

#[component]
pub fn Button(
    on_click: impl Fn(MouseEvent) + 'static,
    children: Children,
    #[prop(default = ButtonVariant::Ghost)]
    variant: ButtonVariant,
    /// Reactive so a dynamic title (e.g. the zoom readout's held-back
    /// note) tracks state; static strings work unchanged.
    #[prop(into, optional)]
    title: Option<Signal<String>>,
    /// Open/pressed state (reactive, so triggers derive it from their open
    /// signal); renders the accent border+text toggle.
    #[prop(into, default = Signal::derive(|| false))]
    active: Signal<bool>,
    /// Reactive so callers can wire `Signal::derive(...)` from their state;
    /// defaults to always-enabled.
    #[prop(into, default = Signal::derive(|| false))]
    disabled: Signal<bool>,
    #[prop(default = ButtonTone::Neutral)]
    tone: ButtonTone,
    /// Compact sizing (h-8, tighter padding, smaller text) for dense rows.
    #[prop(default = false)]
    compact: bool,
    /// Extra classes appended to the computed class string (callers may
    /// override sizing/surface details; do not fork another button).
    #[prop(optional, into)]
    class: Option<String>,
) -> impl IntoView {
    let base = "btn inline-flex items-center justify-center gap-1.5 rounded-lg border font-medium \
                transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent \
                disabled:cursor-not-allowed disabled:opacity-45 whitespace-nowrap";
    let size_class = if compact {
        "h-8 px-2 text-xs"
    } else {
        "h-9 px-2.5 text-sm"
    };
    // One text colour per button: a danger tone replaces the neutral ink.
    let text_color = match tone {
        ButtonTone::Neutral => "text-ink",
        ButtonTone::Danger => "text-red-400",
    };
    let variant_class = match variant {
        ButtonVariant::Toolbar => format!("border-line bg-surface {text_color} hover:bg-line"),
        ButtonVariant::Ghost => format!("border-transparent bg-transparent {text_color} hover:bg-line"),
        ButtonVariant::Primary => "border-transparent bg-accent text-white hover:brightness-110".to_string(),
    };
    let class_extra = class;
    let class = move || {
        let state_class = if active.get() { "border-accent text-accent" } else { "" };
        let extra = class_extra.as_deref().unwrap_or("");
        [base, size_class, variant_class.as_str(), state_class, extra]
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };

    view! {
        <button
            type="button"
            on:click=on_click
            title=move || title.map(|t| t.get())
            prop:disabled=move || disabled.get()
            class=class
        >
            {children()}
        </button>
    }
}
