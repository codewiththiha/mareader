//! Row 1 of the sidebar: close toggle, floating-search toggle, More menu.
//! Always visible while the sidebar is open (not hover-gated) — the sidebar
//! is the native traffic lights' home.

use leptos::prelude::*;

use crate::components::primitives::button::{Button, ButtonVariant};
use crate::components::primitives::icon::{Icon, IconName};
use crate::components::primitives::tooltip::Tooltip;
use crate::state::SidebarMode;
use crate::components::menus::more_menu::MoreMenu;
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
            // Floating-search toggle: raw button so pointerdown can
            // stop propagation (the floating bar's outside-click
            // dismiss listens on window pointerdown, which would
            // otherwise close the bar and let the click re-open it —
            // a one-way toggle).
            <button
                type="button"
                data-search-chrome="true"
                title="Search (Cmd/Ctrl+F)"
                on:pointerdown=move |ev| ev.stop_propagation()
                on:click=move |_| {
                    if reader.search.visible.get() {
                        crate::effects::reader::search::dismiss_search(reader);
                    } else {
                        crate::effects::reader::search::resume_search(reader);
                    }
                }
                class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-transparent bg-transparent text-ink transition-colors hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            >
                <Icon name=IconName::Search size=18 />
            </button>
            <MoreMenu />
        </div>
    }
}
