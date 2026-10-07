//! Row 1 of the sidebar: close, search and settings.

use leptos::prelude::*;

use crate::state::ReaderState;
use app_chrome::icon::{Icon, IconName};
use app_chrome::icon_button::IconButton;
use app_chrome::tooltip::Tooltip;
use app_state::state::SidebarMode;
use app_ui::components::primitives::controls::button::{Button, ButtonVariant};

#[component]
pub(crate) fn SidebarHeader(reader: ReaderState, sidebar: RwSignal<SidebarMode>) -> impl IntoView {
    // The settings modal's open signal, shared through context.
    let settings_open =
        use_context::<RwSignal<bool>>().expect("the reader page provides the settings-open signal");
    // The chrome row's lead: the macOS traffic-light gutter.
    let lead = if app_chrome::platform::is_macos() {
        "pl-[88px]"
    } else {
        "pl-3"
    };

    view! {
        // A drag region; the filled glyph marks "on".
        <div
            class=format!("flex h-12 shrink-0 items-center gap-1 {lead} pr-2")
            data-tauri-drag-region="deep"
        >
            <Tooltip text="Close sidebar">
                <Button
                    on_click=move |_| sidebar.set(SidebarMode::None)
                    variant=ButtonVariant::Ghost
                    title="Close sidebar"
                >
                    <Icon name=IconName::SidebarOpen size=18 />
                </Button>
            </Tooltip>
            // data-search-chrome marks it for the dismiss exclusion.
            <Tooltip text="Search (Cmd/Ctrl+F)">
                <IconButton
                    icon=IconName::Search
                    title="Search (Cmd/Ctrl+F)"
                    data_search_chrome=true
                    on_click=move || {
                        if reader.search.visible.get() {
                            crate::effects::reader::search::dismiss_search(reader);
                        } else {
                            crate::effects::reader::search::resume_search(reader);
                        }
                    }
                />
            </Tooltip>
            <Tooltip text="Reader settings">
                <IconButton
                    icon=IconName::Settings
                    title="Reader settings"
                    on_click=move || settings_open.set(true)
                />
            </Tooltip>
        </div>
    }
}
