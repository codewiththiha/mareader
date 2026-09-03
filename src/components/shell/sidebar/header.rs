//! Row 1 of the sidebar: close toggle, floating-search toggle, settings gear.
//! Always visible while the sidebar is open (not hover-gated) — on macOS,
//! the sidebar is the native traffic lights' home whenever it is painted,
//! docked or floating, so the gutter is not conditional on the layout mode.
//! On the frameless desktops (Windows/Linux) there is nothing in that
//! corner to clear, and the row starts at the same resting padding the
//! title bar uses there.

use leptos::prelude::*;

use crate::components::primitives::button::{Button, ButtonVariant};
use app_chrome::icon::{Icon, IconName};
use app_chrome::icon_button::IconButton;
use app_chrome::tooltip::Tooltip;
use crate::state::SidebarMode;
use crate::state::ReaderState;

#[component]
pub(crate) fn SidebarHeader(
    reader: ReaderState,
    sidebar: RwSignal<SidebarMode>,
) -> impl IntoView {
    // The settings modal's open signal lives at the reader page and is shared
    // through context (see `reader/page.rs`), so this header can open the same
    // modal the 3-dash menu does.
    let settings_open = use_context::<RwSignal<bool>>()
        .expect("the reader page provides the settings-open signal");
    // The chrome row's lead: the 48px traffic-light inset on macOS, the
    // resting 12px everywhere else. Fixed per process, like the split it
    // comes from — no reason for it to be reactive.
    let lead = if app_chrome::platform::is_macos() {
        "pl-[88px]"
    } else {
        "pl-3"
    };

    view! {
        // A drag region so the window stays grabable from the sidebar; the
        // filled panel glyph below marks "sidebar is on".
        <div
            class=format!("flex h-12 shrink-0 items-center gap-1 {lead} pr-2")
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
