//! The library route (`/`): the recent-books shelf with a minimal titlebar —
//! Open + name left, Appearance + More right, plus the built-in pin. No
//! sidebar / zoom / mode / search: those are reader-only, so the shell
//! controller this page provides is rail-less and the bar keeps the full
//! window width, its gutter and its lights.

use leptos::prelude::*;

use crate::components::primitives::hooks::dom::{TOOLBAR_LEADING_ID, TOOLBAR_TRAILING_ID};
use crate::components::primitives::button::{Button, ButtonVariant};
use crate::components::primitives::icon::{Icon, IconName};
use crate::components::primitives::tooltip::Tooltip;
use crate::components::shell::controller::ShellController;
use crate::components::shell::titlebar::app_title_bar::AppTitleBar;
use crate::components::menus::appearance_menu::AppearanceMenu;
use crate::components::shell::titlebar::document_title::DocumentTitle;
use crate::components::menus::app_menu::MoreMenu;
use crate::state::AppState;
use crate::features::library::LibraryShelf;

#[component]
pub fn LibraryPage(state: AppState) -> impl IntoView {
    // The shell's layout truth, answering every rail question with "no
    // rail" — the bar and the traffic lights ask it like any page's do.
    let shell = ShellController::titlebar_only(state);
    provide_context(shell);

    let left = move || {
        view! {
            <div class="flex min-w-0 items-center gap-1">
                <div
                    id=TOOLBAR_LEADING_ID
                    data-tauri-drag-region="true"
                    class="flex shrink-0 items-center gap-1"
                >
                    <Tooltip text="Open PDF (Cmd/Ctrl+O)">
                        <Button
                            on_click=move |_| crate::services::document::open_dialog(state)
                            variant=ButtonVariant::Toolbar
                            title="Open PDF (Cmd/Ctrl+O)"
                        >
                            <Icon name=IconName::Open size=18 />
                            <span>"Open"</span>
                        </Button>
                    </Tooltip>
                </div>
                <DocumentTitle state=state />
            </div>
        }
    };
    let right = move || {
        view! {
            <div
                id=TOOLBAR_TRAILING_ID
                data-tauri-drag-region="true"
                class="flex shrink-0 items-center gap-1"
            >
                <AppearanceMenu state=state />
                <MoreMenu />
            </div>
        }
    };

    view! {
        <AppTitleBar state=state left=left right=right>
            <div class="relative h-full w-full overflow-hidden bg-paper text-ink">
                <LibraryShelf state=state />
            </div>
        </AppTitleBar>
    }
}
