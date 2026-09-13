//! The shelf's right-click: one menu per kind of thing under the pointer.
//!
//! A card used to answer a right-click with the removal receipt and nothing else,
//! which is one row of a menu wearing the whole gesture. The receipt is still what
//! a removal costs and still asks first — it is just reached from a row now, beside
//! the things a right-click is actually for: opening, selecting, renaming the name
//! the shelf shows, duplicating the thing under the pointer as a second instance
//! beside itself, revealing the file in the OS's own manager, finding a book whose
//! address died, taking a shelf apart.
//!
//! One host and one signal, for the reason the removal sheet is one: a right-click
//! can land on a card, a row, a folder or the empty shelf, and four surfaces each
//! owning a menu is four placements, four dismissals and four sets of rows to keep
//! in step. So the surfaces ask and this answers, and the payload says which menu.
//!
//! The primitive underneath is `crate::components::primitives::floating::context_menu`,
//! which owns the cursor placement, the viewport clamp and the dismissal; what is
//! here is the library's half — what a right-click on each kind of thing means.
//!
//! Two things this deliberately does not do. It does not start a drag: a menu row
//! is clicked with a pointer that has already been released, so a session begun
//! from one would have no pointer to follow and no release to end it, and the next
//! click anywhere would be the drop. And it does not fork a second shelf picker —
//! "file these somewhere" is the selection bar's popover, which is on screen
//! whenever a selection is, and a menu that rebuilt it would be a second answer to
//! the same question.

use leptos::prelude::*;

use app_chrome::icon::IconName;

use crate::components::primitives::floating::context_menu::ContextMenu;
use crate::components::primitives::menu::menu_item::{MenuItem, MenuItemTone};
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::menu::separator::Separator;
use crate::features::library::content::{FolderOrder, ShelfOrder};
use crate::features::library::remove_modal::RemoveSheet;
use crate::features::library::rename_modal::RenameSheet;
use crate::features::library::selection::{
    ask_remove_selection, enter_selection, exit_selection, file_selection_on_new_shelf,
    select_on_screen,
};
use crate::services::document;
use crate::services::library::{
    create_shelf_and_enter, delete_shelf, duplicate_row, duplicate_rows, duplicate_shelf,
    path_of_row, path_of_shelf, relink_dialog, reveal_in_folder, set_shelf_watch, shelf_watch,
    ShelfWatch,
};
use crate::state::AppState;

/// What was right-clicked.
///
/// Carrying the facts rather than an id: a menu row that asked the library what it
/// was pointing at would be reading a list that a rescan can change between the
/// click and the row, and the two facts that shape a menu — a book whose address
/// died, a shelf the disk places — are ones the card already knew.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTarget {
    /// A row: a card in the grid or a line in the list, a book or a link. The
    /// id is all an open needs — it is what says WHICH row the reader meant
    /// when the library holds two of one name, and what says whether the thing
    /// clicked is a book to open or a pointer to go to (see
    /// `crate::services::document::open::open_row`) — so carrying the address
    /// beside it would be a second answer to a question the row already
    /// answered. `missing` is a book's fact and a link is never missing: a
    /// pointer at a book that went is a pointer at a book the shelf still
    /// shows, and the book's own row is the one that says so.
    Book { id: String, missing: bool },
    /// A shelf drawn as a folder.
    Folder { id: String },
    /// A card that is already in the selection: the menu acts on the whole set,
    /// which is what a right-click on one of several things means everywhere else.
    Selection,
    /// The empty shelf — the level's own space, with no card under the pointer.
    Level,
}

/// One right-click: where it happened and what it happened on.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuRequest {
    pub x: f64,
    pub y: f64,
    pub target: MenuTarget,
}

/// The host's handle, provided by the page and asked by every surface that can be
/// right-clicked.
#[derive(Clone, Copy)]
pub struct LibraryMenuHost {
    /// The open request, or `None`. One signal for the whole page, so exactly one
    /// menu is up at a time and asking for a second closes the first by replacing
    /// it rather than by either surface knowing about the other.
    pub request: RwSignal<Option<MenuRequest>>,
}

impl LibraryMenuHost {
    /// Create and provide the handle. Called once, by the page.
    pub fn provide() -> Self {
        let this = Self {
            request: RwSignal::new(None),
        };
        provide_context(this);
        this
    }

    /// Ask for a menu at the pointer.
    pub fn ask(&self, x: f64, y: f64, target: MenuTarget) {
        self.request.set(Some(MenuRequest { x, y, target }));
    }
}

/// The shelf's one context menu.
#[component]
pub(crate) fn LibraryContextMenu(state: AppState) -> impl IntoView {
    let menu = use_context::<LibraryMenuHost>().expect("the library page provides the menu");
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");
    let rename_sheet = use_context::<RenameSheet>().expect("the library page provides the sheet");
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let folders = use_context::<FolderOrder>().expect("the library content provides the folders");
    let request = menu.request;
    // Closing is one write, handed to whichever row was chosen rather than left to
    // each menu to remember: a row that acted without closing would leave a menu
    // floating over the shelf it had just changed.
    let close = Callback::new(move |_| request.set(None));

    view! {
        <ContextMenu
            target=request
            position=|at: &MenuRequest| (at.x, at.y)
            on_close=close
            min_width=208
            class="lib-context-menu"
        >
            {move || {
                let Some(at) = request.get() else {
                    return ().into_any();
                };
                match at.target {
                    MenuTarget::Book { id, missing } => {
                        view! {
                            <BookMenu
                                state=state
                                id=id
                                missing=missing
                                rename_sheet=rename_sheet
                                close=close
                            />
                        }
                            .into_any()
                    }
                    MenuTarget::Folder { id } => {
                        view! {
                            <FolderMenu state=state id=id rename_sheet=rename_sheet close=close />
                        }
                            .into_any()
                    }
                    MenuTarget::Selection => {
                        view! { <SelectionMenu state=state remove_sheet=remove_sheet close=close /> }
                            .into_any()
                    }
                    MenuTarget::Level => {
                        view! {
                            <LevelMenu
                                state=state
                                order=order
                                folders=folders
                                close=close
                            />
                        }
                            .into_any()
                    }
                }
            }}
        </ContextMenu>
    }
}

/// A book's menu.
///
/// No row carries a sublabel: a right-click is a reader who knows what the rows
/// mean, and a menu that explains itself on every line is one that has to be
/// read before it can be used.
#[component]
fn BookMenu(
    state: AppState,
    id: String,
    missing: bool,
    rename_sheet: RenameSheet,
    close: Callback<()>,
) -> impl IntoView {
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");
    // Where the file manager would go, read at the build rather than at the
    // click: the store's own copy for a book the library copied, the file
    // where it stands for one read at its place, and the target's own answer
    // for a link. A row with nothing behind it gets no row in the menu — the
    // menu is built per ask, so the answer cannot go stale while it is up.
    let reveal = path_of_row(state, &id);
    // One owned id per row: each row's handler is a closure of its own and a
    // `move` takes what it captures.
    let open_id = id.clone();
    let select_id = id.clone();
    let rename_id = id.clone();
    let dup_id = id.clone();
    let relink_id = id.clone();
    let remove_id = id;

    view! {
        <>
            <MenuItem
                icon=IconName::Open
                label="Open"
                // A book whose address died cannot be opened, and a row that
                // answered with an error toast would be a row that knew better
                // than to be offered.
                disabled=missing
                on_click=move || {
                    close.run(());
                    document::open_row(state, open_id.clone());
                }
            />
            <MenuItem
                icon=IconName::Check
                label="Select"
                on_click=move || {
                    close.run(());
                    enter_selection(state, &select_id);
                }
            />
            <MenuItem
                icon=IconName::Pencil
                label="Rename…"
                // The sheet renames what the shelf SHOWS — a book's title or a
                // link's own name — which is a fact every row has, address or
                // no address: a book whose file died is exactly the book a
                // reader may want to rename before hunting the file down.
                title="The name the library shows — the file on disk keeps its own".to_string()
                on_click=move || {
                    close.run(());
                    rename_sheet.ask_row(state, &rename_id);
                }
            />
            <MenuItem
                icon=IconName::Copy
                label="Duplicate"
                title="A second copy of this book, filed beside it".to_string()
                // A duplicate is a copy, and a book whose address died has
                // nothing to copy — the Open row's own rule.
                disabled=missing
                on_click=move || {
                    close.run(());
                    duplicate_row(state, &dup_id);
                }
            />
            {reveal.map(|path| {
                view! {
                    <MenuItem
                        icon=IconName::Folder
                        label="Reveal in folder"
                        // A book whose address died has nothing to reveal: a
                        // row that would answer with the shell's "not there
                        // any more" is a row that knew better than to be
                        // offered, the Open row's own rule.
                        disabled=missing
                        on_click=move || {
                            close.run(());
                            reveal_in_folder(state, path.clone());
                        }
                    />
                }
            })}
            {missing.then(|| {
                view! {
                    <MenuItem
                        icon=IconName::Search
                        label="Find again…"
                        on_click=move || {
                            close.run(());
                            relink_dialog(state, relink_id.clone());
                        }
                    />
                }
            })}
            <Separator spacing="my-1" />
            <MenuItem
                icon=IconName::Close
                label="Remove from library"
                tone=MenuItemTone::Danger
                on_click=move || {
                    close.run(());
                    remove_sheet.ask(&remove_id);
                }
            />
        </>
    }
}

/// A shelf's menu, drawn as a folder.
///
/// The one place a shelf can be taken apart from without selecting it first, which
/// is the affordance the breadcrumb's parked popover used to be the only one for —
/// and the one place a shelf is subdivided from where it stands: the new shelf is
/// filed inside the one that was asked, whichever level the page is on, because
/// "new shelf" on a folder is an answer about that folder and not about the page.
///
/// It carries the same Duplicate a book's menu does, and the two are one gesture
/// with two answers rather than two gestures: a right-click that duplicated a card
/// and quietly did nothing on a folder is a menu whose rows mean different things
/// depending on what happened to be under the pointer. What differs is the copy —
/// a shelf holds membership and never held a byte, so its duplicate is a second
/// shelf of the reader's own holding the same books, and a folder shelf's copy is
/// the reader's own too rather than a second door to one directory
/// (`crate::services::library::duplicate`).
///
/// It is also the one place a folder's WATCH is turned by hand, and the row is
/// the SEAT's rather than the tree's: tracking is a tree
/// (`library_core::tracking`), so a right-click on a rung shelf turns that rung —
/// an explicit decision at that rung, with the tree above keeping its own — and
/// a right-click on the root shelf turns the whole tree, which is the root's
/// seat. The import sheet's switch asks the same per-rung question of the
/// ground being imported; this row is the answer for a shelf that already
/// stands (`crate::services::library::set_shelf_watch`).
#[component]
fn FolderMenu(
    state: AppState,
    id: String,
    rename_sheet: RenameSheet,
    close: Callback<()>,
) -> impl IntoView {
    // The directory this shelf represents, when it represents one: the ground
    // its watched folder's tree cut it from. A shelf the reader owns has no
    // ground and gets no row.
    let reveal = path_of_shelf(state, &id);
    // The watch this shelf's seat answers for, when it answers for one: a
    // shelf of a folder the library reads in place, at its root or at any rung
    // of its tree, and a shelf a hand made inside one. The decision is the
    // SEAT's — the rung this shelf stands on — so the row toggles the ground
    // the reader is looking at, and the sublabel says which ground that is:
    // the whole folder at the root, only the subfolder at a rung.
    let watch: Option<ShelfWatch> = shelf_watch(state, &id);
    let open_id = id.clone();
    let select_id = id.clone();
    let rename_id = id.clone();
    let dup_id = id.clone();
    let inside_id = id.clone();
    let watch_id = id.clone();
    let remove_id = id;

    view! {
        <>
            <MenuItem
                icon=IconName::Open
                label="Open shelf"
                on_click=move || {
                    close.run(());
                    state.library.shelf.set(open_id.clone());
                }
            />
            <MenuItem
                icon=IconName::Check
                label="Select"
                on_click=move || {
                    close.run(());
                    enter_selection(state, &select_id);
                }
            />
            <MenuItem
                icon=IconName::Pencil
                label="Rename…"
                // The shelf's DISPLAY name: the crumb's inline field renames the
                // same fact, and neither touches a directory on disk — a folder
                // shelf keeps the name its ground has.
                title="The name the library shows — a folder on disk keeps its own".to_string()
                on_click=move || {
                    close.run(());
                    rename_sheet.ask_shelf(state, &rename_id);
                }
            />
            <MenuItem
                icon=IconName::Copy
                label="Duplicate"
                // A shelf holds membership and never held a byte, so its copy is
                // a second shelf of the reader's own with the same books in it —
                // including a folder shelf, which IS the OS directory and cannot
                // be a second door to one directory (`duplicate_shelf_row`).
                title="A second shelf of your own, holding the same books".to_string()
                on_click=move || {
                    close.run(());
                    duplicate_shelf(state, &dup_id);
                }
            />
            {reveal.map(|path| {
                view! {
                    <MenuItem
                        icon=IconName::Folder
                        label="Reveal in folder"
                        on_click=move || {
                            close.run(());
                            reveal_in_folder(state, path.clone());
                        }
                    />
                }
            })}
            {watch.map(|watch| {
                let on = watch.on;
                // The row toggles the SEAT this shelf stands on — the whole
                // tree from the watched root, and only a subfolder and the
                // ground inside it from a rung. The second line is what says
                // which: from three shelves deep, "stop watching" without a
                // named ground is a surprise rather than a toggle.
                let deep = watch.rung_label.is_some();
                let (icon, label) = match (on, deep) {
                    (true, true) => (IconName::EyeOff, "Stop watching this subfolder"),
                    (true, false) => (IconName::EyeOff, "Stop watching for new books"),
                    (false, true) => (IconName::Eye, "Watch this subfolder for new books"),
                    (false, false) => (IconName::Eye, "Watch for new books"),
                };
                let sublabel = match &watch.rung_label {
                    Some(rung) => format!("Only “{rung}” and the folders inside it"),
                    None => format!("The whole “{}” folder", watch.label),
                };
                let title = match (on, deep) {
                    (true, true) => {
                        "Stop checking this subfolder for new books; the books already here \
                         stay, and the rest of the tree keeps watching its own"
                    }
                    (true, false) => "Stop checking this folder; the books already here stay",
                    (false, _) => {
                        "Check this folder for new books when the app opens or you come back to it"
                    }
                }
                .to_string();
                view! {
                    <MenuItem
                        icon=icon
                        label=label
                        sublabel=sublabel
                        title=title
                        on_click=move || {
                            close.run(());
                            set_shelf_watch(state, &watch_id, !on);
                        }
                    />
                }
            })}
            <MenuItem
                icon=IconName::Plus
                label="New shelf"
                on_click=move || {
                    close.run(());
                    create_shelf_and_enter(state, Some(&inside_id));
                }
            />
            <Separator spacing="my-1" />
            <MenuItem
                icon=IconName::Close
                label="Take shelf apart"
                tone=MenuItemTone::Danger
                on_click=move || {
                    close.run(());
                    delete_shelf(state, &remove_id);
                }
            />
        </>
    }
}

/// The set's menu, from a right-click on any card already in it.
///
/// The count is read when the menu is built rather than carried in the request, so
/// the heading and the removal row cannot disagree about how many things they are
/// talking about. It is a number and not a signal because a menu row's label is a
/// `String`: the set cannot change while a menu is up — every action that changes
/// it closes the menu first — and a row that pretended otherwise would be a row
/// the primitive cannot draw.
#[component]
fn SelectionMenu(state: AppState, remove_sheet: RemoveSheet, close: Callback<()>) -> impl IntoView {
    let count = state.library.selected.with_untracked(|set| set.len());
    let heading = format!("{count} selected");
    let remove_label = format!("Remove ({count})");
    let duplicate_label = format!("Duplicate ({count})");

    view! {
        <>
            <SectionLabel text=heading />
            <MenuItem
                icon=IconName::Plus
                label="New shelf from these"
                on_click=move || {
                    close.run(());
                    file_selection_on_new_shelf(state);
                }
            />
            <MenuItem
                icon=IconName::Copy
                label=duplicate_label
                // One act per selected thing, and each is the single duplicate's
                // own — a copy beside the original, in the counter name the level
                // gives it. The set holds both kinds of thing a right-click lands
                // on, so a shelf in it duplicates as a shelf (its own copy, its
                // own subtree) and a book as a book; things that cannot be copied
                // (a book whose file died) are skipped by the service, which is
                // where that fact lives.
                on_click=move || {
                    close.run(());
                    let ids: Vec<String> = state
                        .library
                        .selected
                        .with_untracked(|set| set.iter().cloned().collect());
                    duplicate_rows(state, &ids);
                }
            />
            <Separator spacing="my-1" />
            <MenuItem
                icon=IconName::Close
                label=remove_label
                tone=MenuItemTone::Danger
                on_click=move || {
                    close.run(());
                    ask_remove_selection(state, &remove_sheet);
                }
            />
            <MenuItem
                icon=IconName::Undo
                label="Clear selection"
                on_click=move || {
                    close.run(());
                    exit_selection(state);
                }
            />
        </>
    }
}

/// The level's menu, from a right-click on empty shelf.
#[component]
fn LevelMenu(
    state: AppState,
    order: ShelfOrder,
    folders: FolderOrder,
    close: Callback<()>,
) -> impl IntoView {
    // A row that silently does nothing is worse than no row, so "Select all" is
    // only here while there is something on the level to select.
    let anything = Signal::derive(move || {
        !order.0.with(|books| books.is_empty()) || !folders.0.with(|each| each.is_empty())
    });
    let selecting = state.library.selecting;

    view! {
        <>
            <MenuItem
                icon=IconName::Plus
                label="New shelf"
                on_click=move || {
                    close.run(());
                    create_shelf_and_enter(state, None);
                }
            />
            {move || {
                if !anything.get() {
                    return ().into_any();
                }
                if selecting.get() {
                    return view! {
                        <MenuItem
                            icon=IconName::Undo
                            label="Clear selection"
                            on_click=move || {
                                close.run(());
                                exit_selection(state);
                            }
                        />
                    }
                        .into_any();
                }
                view! {
                    <MenuItem
                        icon=IconName::Check
                        label="Select all"
                        on_click=move || {
                            close.run(());
                            select_on_screen(state, order, folders);
                        }
                    />
                }
                    .into_any()
            }}
        </>
    }
}
