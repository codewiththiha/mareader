//! The library route (`/`): the recent-books shelf with a minimal titlebar —
//! Open + name left, Appearance + More right, plus the built-in pin. No
//! sidebar / zoom / mode / search: those are reader-only.

use leptos::prelude::*;

use pdf_viewer::components::atoms::button::{Button, ButtonKind};
use pdf_viewer::components::atoms::icon::IconName;
use pdf_viewer::components::atoms::tooltip::Tooltip;
use crate::components::chrome::titlebar_provider::TitleBarProvider;
use crate::components::molecules::appearance_menu::AppearanceMenu;
use crate::components::molecules::doc_title::DocTitle;
use crate::components::molecules::more_menu::MoreMenu;
use crate::core::state::AppState;
use super::library_view::LibraryView;

#[component]
pub fn LibraryPage(state: AppState) -> impl IntoView {
    let left = move || {
        view! {
            <div class="flex min-w-0 items-center gap-1">
                <div
                    id="toolbar-left-pre"
                    data-tauri-drag-region="true"
                    class="flex shrink-0 items-center gap-1"
                >
                    <Tooltip text="Open PDF (Cmd/Ctrl+O)".to_string()>
                        <Button
                            on_click=move |_| crate::core::open_flow::open_dialog(state)
                            kind=ButtonKind::Toolbar
                            icon=IconName::Open
                            label="Open".to_string()
                            title="Open PDF (Cmd/Ctrl+O)".to_string()
                        />
                    </Tooltip>
                </div>
                <DocTitle state=state />
            </div>
        }
    };
    let right = move || {
        view! {
            <div
                id="toolbar-right"
                data-tauri-drag-region="true"
                class="flex shrink-0 items-center gap-1"
            >
                <AppearanceMenu state=state />
                <MoreMenu state=state />
            </div>
        }
    };

    view! {
        <TitleBarProvider state=state left=left right=right>
            <div class="relative h-full w-full overflow-hidden bg-paper text-ink">
                <LibraryView state=state />
            </div>
        </TitleBarProvider>
    }
}
