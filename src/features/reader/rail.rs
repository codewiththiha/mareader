//! The reader rail's composition: the shell's aside and its four slots,
//! written once and mounted from one of two places.
//!
//! The two places are not a stylistic choice — see the sidebar family's
//! `overlay.rs` for the stacking-context story that puts the floating rail
//! outside `.reader-bg`. The page mounts [`crate::components::shell::sidebar::push::PushRail`]
//! and [`crate::components::shell::sidebar::overlay::OverlayRail`], each
//! self-gating on the shell controller's layout; this composition renders
//! identically inside either.
//!
//! Every open/close paint fact the panel hosts need — which panel stays
//! painted through a close, whether thumbnail cells may mount, the intro
//! marker — is asked of the `ShellController`, never recomputed here.

use leptos::prelude::*;

use crate::components::shell::controller::ShellController;
use crate::components::shell::sidebar::container::{request_reveal_active, SidebarShell};
use crate::components::shell::sidebar::document_info::BookInfo;
use crate::components::shell::sidebar::header::SidebarHeader;
use crate::components::shell::sidebar::panels::outline_view::SidebarOutline;
use crate::components::shell::sidebar::panels::thumbnails_view::SidebarThumbs;
use crate::components::shell::sidebar::switcher::PanelSwitcher;
use crate::state::{AppState, SidebarMode};

#[component]
pub(crate) fn ReaderRail(
    state: AppState,
    /// The shell's layout truth, owned by the page so both mount points
    /// share one controller (and one open/close machine).
    shell: ShellController,
) -> impl IntoView {
    let vs = state.reader;
    let sidebar = shell.sidebar_mode;

    view! {
        <SidebarShell
            mode=sidebar
            overlay=shell.is_overlay()
            no_slide=shell.no_slide()
            header=move || view! { <SidebarHeader reader=vs sidebar=sidebar /> }
            info_row=move || view! { <BookInfo reader=vs covers=state.library.covers /> }
            panels=move || view! {
                <SidebarOutline
                    state=vs
                    sidebar=sidebar
                    shown=shell.panel_shown(SidebarMode::Outline)
                    outro=shell.panel_outro()
                    intro=shell.panel_intro()
                />
                <SidebarThumbs
                    state=vs
                    sidebar=sidebar
                    // Cells mount with the aside. Cached thumbs blit during the
                    // open motion; cold cells keep their skeleton until their
                    // capped render completes.
                    live=shell.thumbs_live()
                    shown=shell.panel_shown(SidebarMode::Thumbs)
                    outro=shell.panel_outro()
                    intro=shell.panel_intro()
                />
            }
            footer=move || view! {
                <PanelSwitcher
                    mode=sidebar
                    thumbs_active=shell.panel_active(SidebarMode::Thumbs)
                    outline_active=shell.panel_active(SidebarMode::Outline)
                    on_reveal=request_reveal_active
                />
            }
        />
    }
}
