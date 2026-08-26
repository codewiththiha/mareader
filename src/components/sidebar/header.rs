//! Row 1 of the sidebar: close toggle, floating-search toggle, More menu.
//! Always visible while the sidebar is open (not hover-gated) — the sidebar
//! is the native traffic lights' home.

use leptos::prelude::*;

use crate::components::primitives::button::{Button, ButtonVariant};
use crate::components::primitives::icon::{Icon, IconName};
use crate::components::primitives::icon_button::IconButton;
use crate::components::primitives::tooltip::Tooltip;
use crate::state::SidebarMode;
use crate::components::menus::app_menu::MoreMenu;
use crate::state::ReaderState;

#[component]
pub(crate) fn SidebarHeader(
    reader: ReaderState,
    sidebar: RwSignal<SidebarMode>,
) -> impl IntoView {
    view! {
        // The chrome row carries the 48px traffic-light inset; the filled
        // panel glyph marks "sidebar is on". A drag region so the window
        // stays grabable from the sidebar.
        <div
            class="flex h-12 shrink-0 items-center gap-1 pl-[88px] pr-2"
            data-tauri-drag-region="true"
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
            // Floating-search toggle. data-search-chrome marks it for the
            // floating bar's outside-dismiss exclusion, so pointerdown here
            // cannot close the bar the following click is about to toggle —
            // the one-way toggle the raw listener used to enforce via
            // stop_propagation.
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
            <MoreMenu />
        </div>
    }
}
