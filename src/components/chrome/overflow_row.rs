//! Standard overflow-menu row for the adaptive toolbar.

use leptos::prelude::*;

use crate::components::primitives::icon::IconName;
use crate::components::primitives::menu_item::MenuItem;

/// `close_on_click` (default true) dismisses the "…" popover after the
/// action. Set it false for controls that should stay available (zoom ±).
#[component]
pub fn OverflowRow(
    icon: IconName,
    label: &'static str,
    on_click: impl Fn() + 'static,
    done: Callback<()>,
    #[prop(default = true)] close_on_click: bool,
) -> impl IntoView {
    let action = move || {
        on_click();
        if close_on_click {
            done.run(());
        }
    };

    view! {
        <MenuItem icon=icon label=label on_click=action />
    }
}
