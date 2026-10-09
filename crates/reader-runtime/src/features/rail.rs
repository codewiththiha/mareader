//! The reader rail's composition: the aside and its slots.

use leptos::prelude::*;

use crate::components::shell::sidebar::container::{SidebarShell, request_reveal_active};
use crate::components::shell::sidebar::document_info::BookInfo;
use crate::components::shell::sidebar::header::SidebarHeader;
use crate::components::shell::sidebar::panels::dictionary::SidebarDictionary;
use crate::components::shell::sidebar::panels::outline::view::SidebarOutline;
use crate::components::shell::sidebar::panels::thumbnails::view::SidebarThumbs;
use crate::components::shell::sidebar::switcher::PanelSwitcher;
use crate::host::library::LibraryPanel;
use crate::services;
use app_state::SidebarMode;
use app_ui::components::shell::controller::ShellController;

#[component]
pub(crate) fn ReaderRail(
    state: crate::context::ReaderContext,
    /// The shell's layout truth, shared by both mount points.
    shell: ShellController,
) -> impl IntoView {
    let vs = state.reader;
    let sidebar = shell.sidebar_mode;

    // Text documents have no thumbnails; leave the Thumbs panel.
    Effect::new(move |_| {
        if state.reader.reflowable() && sidebar.get_untracked() == SidebarMode::Thumbs {
            sidebar.set(SidebarMode::Outline);
        }
    });
    let thumbs_visible = Signal::derive(move || !vs.reflowable());
    // The workspace's Library panel, owned by the host.
    let library = use_context::<LibraryPanel>();
    // The Dictionary seat: one built pack keeps it open.
    let packs = services::dict::packs();
    let dictionary_visible = Signal::derive(move || packs.get().iter().any(|pack| pack.built));
    Effect::new(move |_| {
        if dictionary_visible.get() {
            return;
        }
        if sidebar.get_untracked() == SidebarMode::Dictionary {
            sidebar.set(SidebarMode::None);
        }
    });

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
                    // Cells mount with the aside.
                    live=shell.thumbs_live()
                    shown=shell.panel_shown(SidebarMode::Thumbs)
                    outro=shell.panel_outro()
                    intro=shell.panel_intro()
                />
                <SidebarDictionary
                    state=state
                    shown=shell.panel_shown(SidebarMode::Dictionary)
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
                    dictionary_visible=dictionary_visible
                    dictionary_active=shell.panel_active(SidebarMode::Dictionary)
                />
            }
        />
    }
}
