//! The library route (`/`): the shelf, and a title bar that is navigation rather
//! than chrome.
//!
//! The bar keeps the reader's shape — leading cluster, centred slot, trailing
//! cluster, the built-in pin — and changes what each is for. Left is where you
//! are (the breadcrumb), centre is how to narrow it (the search), right is how it
//! looks (the view menu) plus the app's own colours (the appearance menu, which
//! drops its page-texture section here: the shelf has no page to texture). Its
//! pin is the library's OWN memory, kept in its own settings field and defaulted
//! to pinned — the shelf's bar is how the reader moves, and unhitching the
//! reader's bar out of a document's way says nothing about it.
//!
//! There is no Open button and no Import button: adding books is a shelf
//! affordance (the `+` card, the empty state, a drop) rather than a title-bar
//! one, because a bar button that opens a picker is a button that hides the
//! library's real door. And no settings button: settings are the reader's —
//! zoom, typography, motion, the AI — and none of them paints a pixel of the
//! shelf, so the gear lives on the bar that has something for it to say.
//!
//! No sidebar, no zoom, no mode, no document search: those are reader-only, so
//! the shell controller this page provides is rail-less and the bar keeps the
//! full window width, its gutter and its lights.

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
use crate::features::library::titlebar_search::TitlebarSearch;
use crate::features::library::view_menu::ViewMenu;
use crate::state::AppState;

#[component]
pub fn LibraryPage(state: AppState) -> impl IntoView {
    // The shell's layout truth, answering every rail question with "no rail" —
    // the bar and the traffic lights ask it like any page's do.
    let shell = ShellController::titlebar_only(state);
    provide_context(shell);

    // The drag session, installed before anything that can be dragged: every
    // card, row and crumb below reads it out of context, and the layer that draws
    // what a drag is holding has to be a sibling of the content rather than a
    // child of it, because a ghost inside a scrolling grid is a ghost that scrolls.
    DragController::install(state);

    // The import sheet's handles, provided here so the three surfaces that can
    // open it (the add card, the empty state, a dropped folder) never have to
    // pass two signals through the grid to reach it.
    let sheet = ImportSheet::provide();
    // The remove sheet's handles, for the same reason: a card and a list row both
    // ask, and neither should have to be told where the sheet lives.
    let remove_sheet = RemoveSheet::provide();
    // The rename sheet's: the ask is the right-click's menu, which is nobody's
    // component child either.
    let rename_sheet = RenameSheet::provide();
    // And the right-click's: a card, a row, a folder and the empty shelf all ask,
    // and one host is what makes the answer the same menu wherever it was asked
    // from. Provided here and rendered by the content, which is where the level's
    // own order lives — a menu row that says "select all" has to mean all of what
    // is on screen.
    LibraryMenuHost::provide();

    // A picker that failed before the sheet could open has nowhere of its own to
    // put the error, and the page owns the app's one toast slot.
    Effect::new(move |_| drain_sheet_toasts(state, sheet));

    let left = move || {
        view! {
            <div class="flex min-w-0 items-center gap-1">
                <div
                    id=TOOLBAR_LEADING_ID
                    data-tauri-drag-region="true"
                    // Squeezable on purpose — no `shrink-0` here. A left
                    // cluster that refuses to shrink answers a long chain by
                    // overflowing OVER the search field, because the row has
                    // no other way to pay for it; `min-w-0` makes the cluster
                    // what gives instead, and the breadcrumb folds itself to
                    // the width it is given (see
                    // `crate::features::library::breadcrumb`). The slot's own
                    // measurement follows the squeeze: the shell observes this
                    // very element.
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
            // #toolbar-trailing is owned by the shell's trailing group (this
            // cluster + the pin), so the page only styles its own cluster here.
            <div data-tauri-drag-region="true" class="flex shrink-0 items-center gap-1">
                <ViewMenu state=state />
                // The surface is asked of the controller rather than named a
                // second time here — `titlebar_only` already said which route
                // this is, and the texture section stands down on the answer.
                <AppearanceMenu state=state surface=shell.surface() />
            </div>
        }
    };

    view! {
        <AppTitleBar state=state left=left center=center right=right>
            <div class="relative h-full w-full overflow-hidden bg-paper text-ink">
                <LibraryContent state=state />
            </div>
            // Fixed and above the content, but below the sheets: a drag is over
            // when a modal opens, and a ghost floating on top of a receipt would
            // be a ghost of something the reader has already put down.
            <DragLayer />
            <ImportModal state=state sheet=sheet />
            <RemoveBookModal state=state sheet=remove_sheet />
            // The display name's sheet: a right-click's Rename for a book, a
            // link or a shelf, and the second door beside the crumb's inline
            // field.
            <RenameModal state=state sheet=rename_sheet />
            // The shelf-already-has-it question. No handle to provide: the
            // sheet opens off a signal on the library state, because the
            // services that raise it — a drop, an import's spawned run — are
            // nobody's component child.
            <ConflictModal state=state />
            // The FOLDER's spelling of the same question, asked before the
            // walk rather than after it: its answers are about a whole import
            // run, not one placement.
            <ShelfConflictModal state=state />
            // The departure's question, asked of a move rather than of an
            // arrival: a read-at-place shelf taken off the seat its folder's
            // tree names becomes the library's own copy, and the copies are a
            // cost the reader is told before they are made.
            <ShelfDepartureModal state=state />
            // Not a question — an answer: a folder the library already reads
            // in place is named, and closing the note lights its shelf up.
            <AlreadyImportedModal state=state />
            // The Find-again question: what a click on a book whose address
            // died asks instead of opening the reader onto an error.
            <RelinkModal state=state />
            // Fixed, and mounted at the page rather than inside the content: an
            // import outlives the state the shelf is in, and a card that unmounted
            // with an "Opening…" would take its progress with it.
            <ProgressDock state=state />
        </AppTitleBar>
    }
}
