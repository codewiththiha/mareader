//! The host's view: the workspace chrome and the slot the panes mount in.
//!
//! The title bar, the backdrop, the rail's two mount points and the settings
//! modal's placement are the host's — ONE copy for the workspace, whatever
//! the panes show. Each chrome region the active pane may fill is a
//! [`ChromeSlot`] the host places and the pane fills through the contract;
//! the workspace slot (`main#viewer-slot`) holds one keyed entry per placed
//! pane.

use leptos::prelude::*;

use super::ReaderHost;
use super::contract::{ChromeSlot, PaneSite};
use super::model::PaneId;
use crate::components::shell::sidebar::overlay::OverlayRail;
use crate::components::shell::sidebar::push::PushRail;
use app_chrome::hooks::dom::{TOOLBAR_LEADING_ID, VIEWER_SLOT_ID};
use app_chrome::icon::{Icon, IconName};
use app_chrome::tooltip::Tooltip;
use app_ui::components::menus::appearance_menu::AppearanceMenu;
use app_ui::components::primitives::controls::button::{Button, ButtonVariant};
use app_ui::components::shell::titlebar::app_title_bar::AppTitleBar;

/// The active pane's contribution to `slot`, placed where this closure
/// runs. Tracked on the active pane only: a focus change re-places the
/// region; nothing inside the pane re-runs it.
fn slot_view(
    host: ReaderHost,
    slot: ChromeSlot,
) -> impl Fn() -> Option<AnyView> + Copy + Send + Sync + 'static {
    move || {
        let pane = host.manager.active_pane()?;
        let site = PaneSite::here();
        untrack(|| pane.chrome(slot, site))
    }
}

/// The workspace: the title bar (sidebar toggle + Library on the left, the
/// active document's title in the centre, its view menu and the host's
/// appearance menu on the right), the backdrop, the rail's mount points, the
/// pane slot, and the settings modal.
#[component]
pub fn ReaderHostView(host: ReaderHost) -> impl IntoView {
    let shell = host.shell;
    let settings = host.session.settings;

    // The sidebar toggle's visibility is the controller's rule: overlay mode
    // drops it (the rail opens by brushing the window's left edge and closes
    // from its own header). The Library button stays put — the rail floats
    // above the bar and covers it while up. The cluster is always mounted so
    // the row keeps its left edge (and `#toolbar-leading`, the measurement
    // anchor the library title uses) wherever the mode puts it.
    let show_sidebar_toggle = move || shell.show_sidebar_toggle().get();
    let has_open_doc = move || host.active_status().holds_document();
    let go_library = move |_| host.return_to_library();
    let left = move || {
        view! {
            <div
                id=TOOLBAR_LEADING_ID
                data-tauri-drag-region="true"
                class="flex shrink-0 items-center gap-1"
            >
                <Show when=show_sidebar_toggle>
                    <Tooltip text="Toggle sidebar">
                        <Button
                            on_click=move |_| shell.toggle_sidebar()
                            variant=ButtonVariant::Ghost
                            title="Toggle sidebar"
                        >
                            <Icon name=IconName::Sidebar size=18 />
                        </Button>
                    </Tooltip>
                </Show>
                <Show when=has_open_doc>
                    <Tooltip text="Library">
                        <Button
                            on_click=go_library
                            variant=ButtonVariant::Ghost
                            title="Close this book and return to the library"
                        >
                            <Icon name=IconName::Library size=18 />
                        </Button>
                    </Tooltip>
                </Show>
            </div>
        }
    };
    let center = slot_view(host, ChromeSlot::TitleCenter);
    let trailing = slot_view(host, ChromeSlot::TitleTrailing);
    let right = move || {
        view! {
            {trailing}
            <AppearanceMenu state=host.chrome />
        }
    };
    let rail_push = slot_view(host, ChromeSlot::Rail);
    let rail_overlay = slot_view(host, ChromeSlot::Rail);
    let settings_modal = slot_view(host, ChromeSlot::Settings);

    // The workspace slot's entries: one per placed pane, keyed by PANE id
    // (never a document id or an index), each positioned at the box the host
    // handed that pane — filling the slot until the first measurement
    // lands. A pane that is not the active one keeps its box (its
    // virtualizers keep measuring) but is hidden and inert; no split mode
    // yet, so every box is the whole slot.
    let manager = host.manager;
    let pane_entry = move |id: PaneId| {
        let Some(pane) = manager.pane(id) else {
            return ().into_any();
        };
        let site = PaneSite::here();
        let content = untrack(|| pane.mount(host.bounds_now(), site));
        // The pane's effects are installed and its view is built (and, off
        // screen, it is parked right away).
        host.pane_ready(id);
        let inactive = move || manager.active() != Some(id);
        let bounds = move || {
            manager
                .bounds_of(id)
                .filter(|b| b.width > 0.0 && b.height > 0.0)
        };
        view! {
            <div
                class="absolute"
                style:left=move || bounds().map_or("0px".to_string(), |b| format!("{}px", b.x))
                style:top=move || bounds().map_or("0px".to_string(), |b| format!("{}px", b.y))
                style:width=move || {
                    bounds().map_or("100%".to_string(), |b| format!("{}px", b.width))
                }
                style:height=move || {
                    bounds().map_or("100%".to_string(), |b| format!("{}px", b.height))
                }
                data-pane-id=id.get()
                data-pane-format=move || {
                    manager
                        .pane(id)
                        .map(|pane| format!("{:?}", pane.format()).to_lowercase())
                        .unwrap_or_default()
                }
                style:visibility=move || if inactive() { "hidden" } else { "visible" }
                style:pointer-events=move || if inactive() { "none" } else { "auto" }
            >
                {content}
            </div>
        }
        .into_any()
    };

    view! {
        <AppTitleBar state=host.chrome left=left center=center right=right>
            // overflow-hidden clips the hidden bottom bar's slide-down
            // translate so it can never leak a phantom scrollbar onto the
            // window.
            <div
                class="reader-bg relative flex h-full w-full flex-col overflow-hidden text-ink"
                class=("blend", move || {
                    // The blend class swaps the backdrop AND the page hosts
                    // onto the engine's one computed paper colour
                    // (styles/components/shell.css, styles/page_host.css),
                    // killing the fractional-edge rim a second paper colour
                    // under the canvas used to show. A text/Markdown page is
                    // its OWN paper (the surface paints --tx-paper), so the
                    // class must not run for it or a second paper colour
                    // stacks under the text page.
                    settings.with(|st| st.layout.blend_mode)
                        && !host.chrome.reader.reflowable.get()
                })
            >
                <div class="relative flex min-h-0 flex-1">
                    // DOCKED: the rail is a flex sibling of `<main>`, so the
                    // workspace gives up the width. `PushRail` renders
                    // nothing while the controller says the layout is
                    // overlay.
                    <PushRail shell=shell>{rail_push}</PushRail>
                    <main
                        id=VIEWER_SLOT_ID
                        class="relative min-w-0 flex-1 overflow-hidden"
                        class=("no-page-shadow", move || !settings.with(|st| st.layout.page_shadow))
                    >
                        <For
                            each=move || manager.placed()
                            key=|id| *id
                            children=pane_entry
                        />
                    </main>
                </div>
            </div>
            // OVERLAY: `OverlayRail` mounts OUTSIDE `.reader-bg`, which is a
            // stacking context at z-index 0 — a rail inside it would paint
            // under the title bar's band. Out here its own z-popover outranks
            // the bar, so the rail covers the bar's left corner (Library
            // button included) and takes the traffic lights with it. Renders
            // nothing while the controller says the layout is docked.
            <OverlayRail shell=shell>{rail_overlay}</OverlayRail>
            // The settings modal belongs to the window, not to the workspace:
            // inside `.reader-bg` (a stacking context) the title bar's band
            // would paint over an open modal. As a sibling, its own z-popover
            // token outranks the bar, and rendering after the floating rail
            // wins their shared token too.
            {settings_modal}
        </AppTitleBar>
    }
}
