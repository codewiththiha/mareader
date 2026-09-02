//! Bottom icon-only rail: Thumbs / Outline panel toggles. Active state is a
//! rounded filled chip, exactly like the reference's bookmark button. Rows
//! are the shared [`ToggleButton`] primitive (size/shape via `variant_class`).

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use crate::components::primitives::toggle_button::ToggleButton;
use crate::state::SidebarMode;

/// One rail toggle: the shared pressed/quiet shell + the rail's own size.
#[component]
fn RailToggle(
    icon: IconName,
    title: &'static str,
    active: Signal<bool>,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    view! {
        <ToggleButton
            active=active
            on_click=on_click
            title=title.to_string()
            variant_class="h-9 w-14"
        >
            <Icon name=icon size=16 />
        </ToggleButton>
    }
}

#[component]
pub(crate) fn PanelSwitcher(
    mode: RwSignal<SidebarMode>,
    thumbs_active: Signal<bool>,
    outline_active: Signal<bool>,
    on_reveal: fn(),
) -> impl IntoView {
    view! {
        <div class="flex shrink-0 items-center justify-around gap-1 border-t border-line p-1.5">
            <RailToggle
                icon=IconName::Thumbs
                title="Thumbnails"
                active=thumbs_active
                on_click=move || {
                    // Re-clicking the ACTIVE tab means "take me to where
                    // I am", not "close".
                    if mode.get() == SidebarMode::Thumbs {
                        on_reveal();
                    } else {
                        mode.set(SidebarMode::Thumbs);
                    }
                }
            />
            <RailToggle
                icon=IconName::Outline
                title="Outline"
                active=outline_active
                on_click=move || {
                    if mode.get() == SidebarMode::Outline {
                        on_reveal();
                    } else {
                        mode.set(SidebarMode::Outline);
                    }
                }
            />
        </div>
    }
}
