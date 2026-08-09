//! Sidebar tab item (icon + label + optional badge). OWNED BY branch C
//! (panels/sidebar).

use leptos::prelude::*;

use crate::components::atoms::icon::IconName;

#[component]
pub fn SidebarItem(
    icon: IconName,
    label: String,
    active: bool,
    badge: Option<String>,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    let _ = (icon, label, active, badge);
    // TODO(branch C): full tab button.
    view! { <div on:click=move |_| on_click() class="cursor-pointer px-3 py-2 text-sm" /> }
}
