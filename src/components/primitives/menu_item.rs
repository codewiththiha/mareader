//! Menu row: icon + label + optional trailing content. Shared by the More
//! menu rows and the toolbar's overflow rows (`OverflowRow`).

use leptos::prelude::*;

use crate::components::primitives::icon::{Icon, IconName};

#[component]
pub fn MenuItem(
    icon: IconName,
    #[prop(into)] label: String,
    on_click: impl Fn() + 'static,
    #[prop(optional)]
    children: Option<Children>,
) -> impl IntoView {
    view! {
        <button
            type="button"
            on:click=move |_| on_click()
            class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm text-ink hover:bg-line"
        >
            <span class="inline-flex w-4 shrink-0 justify-center text-muted"><Icon name=icon size=14 /></span>
            <span>{label}</span>
            {children.map(|c| c())}
        </button>
    }
}
