//! Icon-only button: the toolbar's square ghost button. `pressed` renders
//! the accent/ink toggle (titlebar pin); `disabled` is the plain
//! disabled:opacity-50 the other controls use.
//!
//! The borderless look is deliberate and shared: toolbar icon buttons sit
//! on glass/paper and only lift on hover, while the bordered
//! [`Button`](super::button::Button) variant is for labeled actions.

use leptos::prelude::*;

use super::icon::{Icon, IconName};

#[component]
pub fn IconButton(
    /// Leading icon, unless `children` (dynamic icons) are supplied.
    #[prop(optional, into)]
    icon: Option<IconName>,
    on_click: impl Fn() + 'static,
    #[prop(into, optional)]
    title: Option<String>,
    #[prop(default = 18)]
    size: u16,
    /// Optional toggle state: renders `aria-pressed` plus the accent/ink
    /// colour swap (the titlebar pin).
    #[prop(optional)]
    pressed: Option<Signal<bool>>,
    /// Reactive so callers can wire `Signal::derive(...)` from their state;
    /// defaults to always-enabled.
    #[prop(into, default = Signal::derive(|| false))]
    disabled: Signal<bool>,
    /// Marks the button as search chrome for the floating search's
    /// outside-dismiss exclusion (pointerdown on this button must not close
    /// the bar the click is about to toggle). Rendered as
    /// `data-search-chrome="true"`, matching the exclusion selector.
    #[prop(default = false)]
    data_search_chrome: bool,
    /// Extra classes appended to the computed class string (callers may
    /// override colour/size details; do not fork another button).
    #[prop(optional, into)]
    class: Option<String>,
    /// Optional content replacing the icon (dynamic icons: show-results
    /// toggle, …). When absent the `icon` prop renders.
    #[prop(optional)]
    children: Option<Children>,
) -> impl IntoView {
    let pressed_sig = pressed.unwrap_or_else(|| Signal::derive(|| false));
    let box_class = "btn-icon inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-lg";
    // Plain (non-toggle) buttons carry no text colour class of their own —
    // they inherit, so a caller's `class` passthrough can set the colour
    // without fighting a conditional utility. Toggle buttons keep the
    // accent/ink swap (only one branch is ever in the DOM).
    let base_sig = pressed.map(|_| {
        (
            format!(
                "{box_class} border border-transparent bg-transparent transition-colors hover:bg-line \
                 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent \
                 disabled:pointer-events-none disabled:opacity-50"
            ),
            pressed_sig,
        )
    });
    let base_plain = format!(
        "{box_class} border border-transparent bg-transparent transition-colors hover:bg-line \
         focus:outline-none focus-visible:ring-2 focus-visible:ring-accent \
         disabled:pointer-events-none disabled:opacity-50"
    );

    view! {
        <button
            type="button"
            title=title
            data-search-chrome=move || if data_search_chrome { "true" } else { "" }
            aria-pressed=pressed.map(|_| move || pressed_sig.get().to_string())
            prop:disabled=move || disabled.get()
            on:click=move |_| on_click()
            class=move || {
                let extra = class.as_deref().unwrap_or("");
                match &base_sig {
                    Some((base, ps)) => {
                        format!("{base} {} {extra}", if ps.get() { "text-accent" } else { "text-ink" })
                    }
                    None => format!("{base_plain} {extra}"),
                }
            }
        >
            {match (children, icon) {
                (Some(c), _) => c(),
                (None, Some(i)) => view! { <Icon name=i size=size /> }.into_any(),
                (None, None) => ().into_any(),
            }}
        </button>
    }
}
