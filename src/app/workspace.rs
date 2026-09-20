use leptos::prelude::*;
use crate::state::AppState;
use crate::components::shell::controller::ShellController;
use crate::components::shell::sidebar::overlay::OverlayRail;
use crate::components::shell::sidebar::push::PushRail;
use crate::components::shell::titlebar::app_title_bar::AppTitleBar;
use crate::components::shell::titlebar::document_title::CenteredDocTitle;
use crate::components::menus::appearance_menu::AppearanceMenu;
use crate::components::menus::reader_menu::ReaderMenu;
use crate::components::settings::modal::SettingsModal;
use crate::components::primitives::controls::button::{Button, ButtonVariant};
use app_chrome::hooks::dom::TOOLBAR_LEADING_ID;
use app_chrome::icon::{Icon, IconName};
use app_chrome::tooltip::Tooltip;
use pdf_engine::types::DocStatus;

#[component]
pub(super) fn WorkspaceShell(state: AppState) -> impl IntoView {
    let shell = ShellController::reader(state);
    provide_context(shell);
    let settings_open = RwSignal::new(false);
    provide_context(settings_open);
    provide_context(RwSignal::new("library"));
    // Left: sidebar toggle + Library; title centered; right: the 3-dash view
    // menu + Appearance.
    //
    // The sidebar toggle's visibility is the controller's rule: overlay mode
    // drops it (the rail opens by brushing the window's left edge and closes
    // from its own header, so a second switch in the bar only competes with
    // both). The Library button stays put — the rail floats above the bar and
    // covers it while up, which is the rail's job. The cluster is always
    // mounted so the row keeps its left edge (and `#toolbar-leading`, the
    // measurement anchor the library title uses) wherever the mode puts it.
    // Reader settings have no button of their own: they open from the 3-dash
    // menu's Settings item and the sidebar header's gear.
    let left = move || {
        view! {
            <div
                id=TOOLBAR_LEADING_ID
                data-tauri-drag-region="true"
                class="flex shrink-0 items-center gap-1"
            >
                <Show when=move || shell.show_sidebar_toggle().get()>
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
                <Show when=move || {
                    matches!(
                        state.reader.document.status.get(),
                        DocStatus::Ready | DocStatus::Opening
                    )
                }>
                    <Tooltip text="Library">
                        <Button
                            on_click=move |_| crate::runtime::emit(serde_json::json!({"type":"close-all"}))
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
    let center = move || view! { <CenteredDocTitle state=state /> };
    let right = move || {
        view! {
            <ReaderMenu state=state settings_open=settings_open />
            <AppearanceMenu state=state />
        }
    };


    view! {
        <AppTitleBar state=state left=left center=center right=right>
            <ChromeRelay shell=shell />
            <div class="reader-bg relative flex h-full w-full overflow-hidden text-ink">
                <PushRail shell=shell><WorkspaceRail state=state shell=shell /></PushRail>
                <main id="workspace-layout" class="relative min-w-0 flex-1 overflow-hidden"></main>
            </div>
            <OverlayRail shell=shell><WorkspaceRail state=state shell=shell /></OverlayRail>
            <SettingsModal state=state open=settings_open />
        </AppTitleBar>
    }
}

#[component]
fn WorkspaceRail(state: AppState, shell: ShellController) -> impl IntoView {
    use crate::components::shell::sidebar::{container::SidebarShell, header::SidebarHeader, document_info::BookInfo};
    use crate::components::shell::sidebar::panels::outline::view::SidebarOutline;
    use crate::components::shell::sidebar::panels::thumbnails::view::SidebarThumbs;
    use crate::components::shell::sidebar::switcher::PanelSwitcher;
    use crate::state::SidebarMode;
    let tab = expect_context::<RwSignal<&'static str>>();
    view! {
        <SidebarShell mode=shell.sidebar_mode overlay=shell.is_overlay() no_slide=shell.no_slide()
            header=move || view! { <SidebarHeader reader=state.reader sidebar=shell.sidebar_mode /> }
            info_row=move || view! { <BookInfo reader=state.reader cover=state.reader.cover /> }
            panels=move || view! {
                <div class="workspace-sidebar h-full overflow-auto" hidden=move || tab.get() != "library" />
                <Show when=move || tab.get() == "thumbnails">
                    <SidebarThumbs state=state.reader sidebar=shell.sidebar_mode live=shell.thumbs_live()
                        shown=Signal::derive(|| true) outro=shell.panel_outro() intro=shell.panel_intro() />
                </Show>
                <Show when=move || tab.get() == "outline">
                    <SidebarOutline state=state.reader sidebar=shell.sidebar_mode
                        shown=Signal::derive(|| true) outro=shell.panel_outro() intro=shell.panel_intro() />
                </Show>
            }
            footer=move || view! {
                <PanelSwitcher mode=shell.sidebar_mode
                    thumbs_active=Signal::derive(move || tab.get() == "thumbnails")
                    outline_active=Signal::derive(move || tab.get() == "outline")
                    library_active=Signal::derive(move || tab.get() == "library")
                    on_reveal=crate::components::shell::sidebar::container::request_reveal_active
                    on_library=Callback::new(move |()| {
                        tab.set("library");
                        crate::runtime::emit(serde_json::json!({"type":"sidebar-tab","tab":"library"}));
                    })
                    on_panel=Callback::new(move |mode| {
                        let name = if mode == SidebarMode::Thumbs { "thumbnails" } else { "outline" };
                        tab.set(name);
                        crate::runtime::emit(serde_json::json!({"type":"sidebar-tab","tab":name}));
                    })
                />
            }
        />
    }
}

#[component]
fn ChromeRelay(shell: ShellController) -> impl IntoView {
    let bar = expect_context::<app_chrome::titlebar::root::TitleBarCtx>();
    Effect::new(move |_| crate::runtime::emit(serde_json::json!({
        "type":"chrome-state", "bar":bar.visible.get(), "rail":shell.rail_present().get(),
    })));
}
