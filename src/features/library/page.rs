//! The library route (`/`): the shelf, and a title bar that is navigation rather than chrome.
//!
//! Left is where you are (the breadcrumb), centre is how to narrow it (the search), right is
//! how it looks (the view menu) plus the app's own colours (the appearance menu).

use leptos::prelude::*;

use app_chrome::hooks::dom::TOOLBAR_LEADING_ID;

use crate::components::menus::appearance_menu::AppearanceMenu;
use crate::components::shell::controller::ShellController;
use crate::components::shell::titlebar::app_title_bar::AppTitleBar;
use crate::features::library::breadcrumb::Breadcrumb;
use crate::features::library::conflict_modal::{ConflictModal, ShelfConflictModal};
use crate::features::library::content::LibraryContent;
use crate::features::library::context_menu::LibraryMenuHost;
use crate::features::library::dnd::controller::DragController;
use crate::features::library::already_imported_modal::AlreadyImportedModal;
use crate::features::library::departure_modal::ShelfDepartureModal;
use crate::features::library::dnd::layer::DragLayer;
use crate::features::library::import_modal::{ImportModal, ImportSheet, drain_sheet_toasts};
use crate::features::library::progress_dock::ProgressDock;
use crate::features::library::relink_modal::RelinkModal;
use crate::features::library::remove_modal::{RemoveBookModal, RemoveSheet};
use crate::features::library::rename_modal::{RenameModal, RenameSheet};
use crate::features::library::shelf_apart_modal::ShelfApartModal;
use crate::features::library::titlebar_search::TitlebarSearch;
use crate::features::library::view_menu::ViewMenu;
use crate::state::AppState;

#[component]
pub fn LibraryPage(state: AppState) -> impl IntoView {
    let shell = ShellController::titlebar_only(state);
    provide_context(shell);

    // Installed before anything that can be dragged, and a sibling of the content rather than a
    // child of it, because a ghost inside a scrolling grid is a ghost that scrolls.
    DragController::install(state);

    // Provided here so the three surfaces that can open it (the add card, the empty state, a dropped folder) never have to pass two signals through the grid to reach it.
    let sheet = ImportSheet::provide();
    let remove_sheet = RemoveSheet::provide();
    let rename_sheet = RenameSheet::provide();
    // Provided here and rendered by the content, which is where the level's own order lives — a menu row that says "select all" has to mean all of what is on screen.
    LibraryMenuHost::provide();

    Effect::new(move |_| drain_sheet_toasts(state, sheet));

    let left = move || {
        view! {
            <div class="flex min-w-0 items-center gap-1">
                <div
                    id=TOOLBAR_LEADING_ID
                    data-tauri-drag-region="true"
                    // Squeezable on purpose: a left cluster that refuses to shrink answers a long chain by
                    // overflowing OVER the search field, so `min-w-0` makes the cluster what gives instead and the
                    // breadcrumb folds itself to the width it is given (see `crate::features::library::breadcrumb`).
                    class="flex min-w-0 items-center gap-1"
                >
                    <Breadcrumb state=state />
                </div>
            </div>
        }
    };
    let center = move || view! { <TitlebarSearch state=state /> };
    let right = move || {
        view! {
            <div data-tauri-drag-region="true" class="flex shrink-0 items-center gap-1">
                <ViewMenu state=state />
                <AppearanceMenu state=state surface=shell.surface() />
            </div>
        }
    };

    view! {
        <AppTitleBar state=state left=left center=center right=right>
            <div class="relative h-full w-full overflow-hidden bg-paper text-ink">
                <LibraryContent state=state />
            </div>
            // A drag is over when a modal opens, and a ghost floating on top of a receipt would be a ghost of something the reader has already put down.
            <DragLayer />
            <ImportModal state=state sheet=sheet />
            <RemoveBookModal state=state sheet=remove_sheet />
            <RenameModal state=state sheet=rename_sheet />
            <ConflictModal state=state />
            <ShelfConflictModal state=state />
            <ShelfDepartureModal state=state />
            <ShelfApartModal state=state />
            <AlreadyImportedModal state=state />
            <RelinkModal state=state />
            <ProgressDock state=state />
        </AppTitleBar>
    }
}
