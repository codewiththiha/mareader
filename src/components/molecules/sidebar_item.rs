//! Sidebar tab item (icon + label + optional badge). OWNED BY branch C
//! (panels/sidebar).

use leptos::prelude::*;

use crate::components::atoms::icon::{Icon, IconName};

#[component]
pub fn SidebarItem(
    icon: IconName,
    label: String,
    active: bool,
    #[prop(optional)]
    badge: Option<String>,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    let base = "flex w-full items-center gap-2 border-l-2 px-3 py-2 text-left text-sm transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";
    let state_cls = if active {
        "border-accent bg-line text-ink"
    } else {
        "border-transparent text-muted hover:bg-line hover:text-ink"
    };
    let title_attr = label.clone();
    view! {
        <button
            type="button"
            on:click=move |_| on_click()
            class=format!("{base} {state_cls}")
            title=title_attr
        >
            <Icon name=icon size=16 />
            <span class="min-w-0 flex-1 truncate">{label}</span>
            {badge
                .and_then(|b| (!b.is_empty()).then_some(b))
                .map(|b| view! {
                    <span class="rounded-full bg-accent px-1.5 py-0.5 text-[10px] font-medium leading-none text-white">{b}</span>
                })}
        </button>
    }
}
