//! Bottom icon-only rail: Thumbs / Outline / Library / Dictionary.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use app_state::state::SidebarMode;
use app_ui::components::primitives::controls::toggle_button::{ToggleButton, ToggleVariant};

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
            variant=ToggleVariant::Filled
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
    /// The workspace's Library panel toggle; absent where no workspace
    /// hosts the panel.
    library_active: Option<Signal<bool>>,
    on_reveal: fn(),
    /// The Thumbs toggle exists only while the engine has pages to thumb.
    #[prop(into, default = Signal::derive(|| true))]
    thumbs_visible: Signal<bool>,
    /// The Dictionary toggle's seat: one pack built is enough.
    #[prop(into, default = Signal::derive(|| false))]
    dictionary_visible: Signal<bool>,
    #[prop(into)]
    dictionary_active: Signal<bool>,
) -> impl IntoView {
    view! {
        <div class="flex shrink-0 items-center justify-around gap-1 border-t border-line p-1.5">
            <Show when=move || thumbs_visible.get()>
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
            </Show>
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
            {library_active.map(|active| view! {
                <RailToggle
                    icon=IconName::Library
                    title="Library"
                    active=active
                    on_click=move || mode.set(SidebarMode::Library)
                />
            })}
            <Show when=move || dictionary_visible.get()>
                <RailToggle
                    icon=IconName::Search
                    title="Dictionary"
                    active=dictionary_active
                    on_click=move || mode.set(SidebarMode::Dictionary)
                />
            </Show>
        </div>
    }
}
