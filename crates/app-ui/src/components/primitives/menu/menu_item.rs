//! Menu row: icon, label, optional trailing content.

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

#[component]
pub fn MenuItem(
    /// Leading icon; `None` still renders the aligned slot.
    #[prop(optional, into)]
    icon: Option<IconName>,
    #[prop(into)] label: String,
    on_click: impl Fn() + 'static,
    /// Danger rows read in the destructive colour.
    #[prop(default = MenuItemTone::Default)]
    tone: MenuItemTone,
    #[prop(default = false)] disabled: bool,
    /// Selected/pressed state (checked rows, active options).
    #[prop(optional)]
    selected: Option<Signal<bool>>,
    /// Draw the trailing check from `selected`.
    #[prop(default = false)]
    check: bool,
    /// A muted second line under the label.
    #[prop(optional, into)]
    sublabel: Option<String>,
    /// A tooltip, for rows the label and second line leave short.
    #[prop(optional, into)]
    title: Option<String>,
    /// Row geometry override (denser/larger rows). Defaults to the shared
    /// menu-row look (`rounded-md px-2 py-1.5`).
    #[prop(optional)]
    row_class: Option<&'static str>,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    let selected_sig = selected.unwrap_or_else(|| Signal::derive(|| false));
    let danger = tone == MenuItemTone::Danger;
    let row_class = row_class.unwrap_or("rounded-md px-2 py-1.5");

    // A computed class string: avoids two text-colour utilities fighting.
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
            title=title
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
            {match sublabel {
                Some(sub) => {
                    view! {
                        <span class="min-w-0 flex-1 text-left">
                            <span class="block truncate">{label}</span>
                            <span class="block truncate text-[11px] font-normal text-muted">
                                {sub}
                            </span>
                        </span>
                    }
                        .into_any()
                }
                None => view! { <span>{label}</span> }.into_any(),
            }}
            {children.map(|c| c())}
            {check.then(|| {
                view! {
                    <span class="ml-auto inline-flex w-4 shrink-0 justify-center text-accent">
                        {move || {
                            selected_sig
                                .get()
                                .then(|| view! { <Icon name=IconName::Check size=14 /> })
                        }}
                    </span>
                }
            })}
        </button>
    }
}
