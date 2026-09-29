//! The reader rail's composition: the shell's aside and its four slots,
//! written once and mounted from one of two places.
//!
//! The two places are not a stylistic choice — see the sidebar family's
//! `overlay.rs` for the stacking-context story that puts the floating rail
//! outside `.reader-bg`. The reader host mounts
//! [`crate::components::shell::sidebar::push::PushRail`] and
//! [`crate::components::shell::sidebar::overlay::OverlayRail`], each
//! self-gating on the shell controller's layout, and the active pane fills
//! both through its rail slot; this composition renders identically inside
//! either.
//!
//! Every open/close paint fact the panel hosts need — which panel stays
//! painted through a close, whether thumbnail cells may mount, the intro
//! marker — is asked of the `ShellController`, never recomputed here.

use leptos::prelude::*;

use crate::components::shell::sidebar::container::{SidebarShell, request_reveal_active};
use crate::components::shell::sidebar::document_info::BookInfo;
use crate::components::shell::sidebar::header::SidebarHeader;
use crate::components::shell::sidebar::panels::outline::view::SidebarOutline;
use crate::components::shell::sidebar::panels::thumbnails::view::SidebarThumbs;
use crate::components::shell::sidebar::switcher::PanelSwitcher;
use crate::host::library::LibraryPanel;
use app_state::SidebarMode;
use app_ui::components::shell::controller::ShellController;

#[component]
pub(crate) fn ReaderRail(
    state: crate::context::ReaderContext,
    /// The shell's layout truth, owned by the host so both mount points
    /// share one controller (and one open/close machine).
    shell: ShellController,
) -> impl IntoView {
    let vs = state.reader;
    let sidebar = shell.sidebar_mode;

    // Text documents have no thumbnails — the engine never sees them. If
    // one opens while the rail is ON the Thumbs panel, move the rail to
    // the Outline panel (which degrades gracefully to its empty state) so
    // the reader never faces a panel that cannot show anything.
    Effect::new(move |_| {
        if state.reader.reflowable() && sidebar.get_untracked() == SidebarMode::Thumbs {
            sidebar.set(SidebarMode::Outline);
        }
    });
    let thumbs_visible = Signal::derive(move || !vs.reflowable());
    // The workspace's Library panel, when a host provides one: the host
    // owns it (its tree and open folders outlive this rail, which remounts
    // with the active pane); the rail only gives it a slot and a toggle.
    let library = use_context::<LibraryPanel>();

    view! {
        <SidebarShell
            mode=sidebar
            overlay=shell.is_overlay()
            no_slide=shell.no_slide()
            header=move || view! { <SidebarHeader reader=vs sidebar=sidebar /> }
            info_row=move || view! { <BookInfo reader=vs cover=state.launch.with(|l| l.cover_data_url.clone()) /> }
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
                {library.map(|panel| {
                    let shown = shell.panel_shown(SidebarMode::Library);
                    let outro = shell.panel_outro();
                    let intro = shell.panel_intro();
                    view! {
                        <div
                            class="sidebar-panel absolute inset-0 flex flex-col"
                            class=("invisible", move || !shown.get())
                            class=("is-outro", move || outro.get())
                            class=("is-intro", move || intro.get())
                        >
                            {panel.view(shown)}
                        </div>
                    }
                })}
            }
            footer=move || view! {
                <PanelSwitcher
                    mode=sidebar
                    thumbs_active=shell.panel_active(SidebarMode::Thumbs)
                    outline_active=shell.panel_active(SidebarMode::Outline)
                    library_active=library.map(|_| shell.panel_active(SidebarMode::Library))
                    on_reveal=request_reveal_active
                    thumbs_visible=thumbs_visible
                />
            }
        />
    }
}
