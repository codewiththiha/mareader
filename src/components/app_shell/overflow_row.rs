//! Standard overflow-menu row for the adaptive toolbar.

use leptos::prelude::*;

use crate::components::primitives::icon::IconName;
use crate::components::primitives::menu_item::MenuItem;

/// A toolbar action collapsed into the "…" popover: run the action, then
/// dismiss the popover. Controls that need to stay open after a click
/// simply render their own row instead of reusing this one.
#[component]
pub fn OverflowRow(
    icon: IconName,
    label: &'static str,
    on_click: impl Fn() + 'static,
    done: Callback<()>,
) -> impl IntoView {
    let action = move || {
        on_click();
        done.run(());
    };

    view! {
        <MenuItem icon=icon label=label on_click=action />
    }
}
