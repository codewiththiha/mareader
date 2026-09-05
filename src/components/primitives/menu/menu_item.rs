//! Menu row: icon + label + optional trailing content. Shared by the reader
//! menu's rows and the gloss context menu (where the danger row and the
//! disabled rows live).

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};

/// Semantic tone of a menu row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MenuItemTone {
    #[default]
    Default,
    /// Destructive action (Remove…, Delete…).
    Danger,
}

/// A menu row.
#[component]
pub fn MenuItem(
    /// Leading icon, if any. `None` still renders the aligned w-4 slot so
    /// rows in one menu line up whether or not they carry an icon.
    #[prop(optional, into)]
    icon: Option<IconName>,
    #[prop(into)] label: String,
    on_click: impl Fn() + 'static,
    /// Danger rows read in the destructive colour.
    #[prop(default = MenuItemTone::Default)]
    tone: MenuItemTone,
    #[prop(default = false)]
    disabled: bool,
    /// Selected/pressed state (checked rows, active options).
    #[prop(optional)]
    selected: Option<Signal<bool>>,
    /// Row geometry override (denser/larger rows). Defaults to the shared
    /// menu-row look (`rounded-md px-2 py-1.5`).
    #[prop(optional)]
    row_class: Option<&'static str>,
    #[prop(optional)]
    children: Option<Children>,
) -> impl IntoView {
    let selected_sig = selected.unwrap_or_else(|| Signal::derive(|| false));
    let danger = tone == MenuItemTone::Danger;
    let row_class = row_class.unwrap_or("rounded-md px-2 py-1.5");

    // Computed class string (the repo rule only restricts single-token
    // conditional tuples; a computed string is allowed and avoids two
    // text-colour utilities fighting each other). Selected uses the same
    // accent-soft treatment as OptionButton, so every
    // selected/pressed row in the app speaks one visual language.
    let class = move || {
        let hover = if disabled { "" } else { "hover:bg-line" };
        let base = format!(
            "menu-item flex w-full items-center gap-2 {row_class} text-sm {hover} \
             disabled:cursor-not-allowed disabled:opacity-45"
        );
        if selected_sig.get() && !danger {
            format!("{base} bg-accent-soft font-medium text-accent")
        } else if danger {
            format!("{base} text-red-400")
        } else {
            format!("{base} text-ink")
        }
    };

    view! {
        <button
            type="button"
            role="menuitem"
            disabled=disabled
            aria-disabled=disabled.to_string()
            on:click=move |_| on_click()
            class=class
        >
            <span class="inline-flex w-4 shrink-0 justify-center">
                {icon.map(|i| {
                    view! {
                        <span class=if danger { "text-red-400" } else { "text-muted" }>
                            <Icon name=i size=14 />
                        </span>
                    }
                })}
            </span>
            <span>{label}</span>
            {children.map(|c| c())}
        </button>
    }
}
