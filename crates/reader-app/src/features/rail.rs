//! The reader rail's composition: the shell's aside and its four slots,
//! written once and mounted from one of two places.
//!
//! The two places are not a stylistic choice — see the sidebar family's
//! `overlay.rs` for the stacking-context story that puts the floating rail
//! outside `.reader-bg`. The page mounts [`crate::components::rail::push::PushRail`]
//! and [`crate::components::rail::overlay::OverlayRail`], each
//! self-gating on the shell controller's layout; this composition renders
//! identically inside either.
//!
//! Every open/close paint fact the panel hosts need — which panel stays
//! painted through a close, whether thumbnail cells may mount, the intro
//! marker — is asked of the `ShellController`, never recomputed here.

use leptos::prelude::*;

use app_chrome::controller::ShellController;
use crate::components::rail::container::{SidebarShell, request_reveal_active};
use crate::components::rail::document_info::BookInfo;
use crate::components::rail::header::SidebarHeader;
use crate::components::rail::panels::outline::view::SidebarOutline;
use crate::components::rail::panels::thumbnails::view::SidebarThumbs;
use crate::components::rail::switcher::PanelSwitcher;
use crate::state::ReaderState;
use library_core::covers::CoverMap;
use reader_core::ui::SidebarMode;

#[component]
pub(crate) fn ReaderRail(
    reader: ReaderState,
    /// The library's cover map, for the identity row's thumbnail. Library
    /// data, but rendered reader-side — handed over, not reached for.
    covers: RwSignal<CoverMap>,
    /// The shell's layout truth, owned by the page so both mount points
    /// share one controller (and one open/close machine).
    shell: ShellController,
) -> impl IntoView {
    let vs = reader;
    let sidebar = shell.sidebar_mode;

    // Text documents have no thumbnails — the engine never sees them. If
    // one opens while the rail is ON the Thumbs panel, move the rail to
    // the Outline panel (which degrades gracefully to its empty state) so
    // the reader never faces a panel that cannot show anything.
    Effect::new(move |_| {
        if reader.reflowable()
            && sidebar.get_untracked() == SidebarMode::Thumbs
        {
            sidebar.set(SidebarMode::Outline);
        }
    });
    let thumbs_visible = Signal::derive(move || !vs.reflowable());

    view! {
        <SidebarShell
            mode=sidebar
            overlay=shell.is_overlay()
            no_slide=shell.no_slide()
            header=move || view! { <SidebarHeader reader=vs sidebar=sidebar /> }
            info_row=move || view! { <BookInfo reader=vs covers=covers /> }
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
                    thumbs_visible=thumbs_visible
                />
            }
        />
    }
}
