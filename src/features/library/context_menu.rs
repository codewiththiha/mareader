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
//! Four menus, one renderer and one list of rows. A row is a `MenuItemSpec` —
//! what it says, what it runs, whether it can run, and whether a rule stands
//! above it — and the five rows a book, a link and a folder all answer with are
//! built once, in the one order, by `base_entry_items`. What each menu is, then,
//! is the rows it adds to those five: the row a dead address owns, the watch row
//! a seat answers for, the set's counted rows, the level's conditional one. The
//! two entry menus used to hold the same five rows twice, in two orders, with
//! two disabled rules and two different removal rows for one gesture.
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

/// What one menu row is, as data.
///
/// The rows of the four menus here are the same rows in different orders —
/// open, select, rename, duplicate, reveal, and then whatever the thing under
/// the pointer owns — and a `<MenuItem>` written per menu meant the order, the
/// disabled rule and the removal's own rule were each repeated wherever a row
/// happened to be needed. As a list, a row can be built where the facts are,
/// positioned, and left out; the renderer is then one component rather than
/// four spellings of the same seven props.
struct MenuItemSpec {
    icon: IconName,
    label: String,
    run: Callback<()>,
    sublabel: Option<String>,
    title: Option<String>,
    disabled: bool,
    tone: MenuItemTone,
    /// A rule above this row: the row that takes something away is a different
    /// kind of thing from the rows that change the library, and the rule is
    /// what says so — once, rather than per menu.
    ruled: bool,
}

impl MenuItemSpec {
    /// A row with the defaults a row usually has: no second line, no tooltip,
    /// enabled, ordinary tone, no rule above it.
    fn new(icon: IconName, label: impl Into<String>, run: Callback<()>) -> Self {
        Self {
            icon,
            label: label.into(),
            run,
            sublabel: None,
            title: None,
            disabled: false,
            tone: MenuItemTone::Default,
            ruled: false,
        }
    }

    /// The muted second line, for a row that has something to say about itself
    /// beyond its name.
    fn sublabel(mut self, text: String) -> Self {
        self.sublabel = Some(text);
        self
    }

    /// The tooltip, for a row whose label and second line still leave something
    /// worth saying that fits on neither.
    fn title(mut self, text: &'static str) -> Self {
        self.title = Some(text.to_string());
        self
    }

    /// Stand the row down when the thing it acts on cannot be acted on — a
    /// book whose address died. The row STAYS: a menu whose rows disappeared
    /// with the card's luck would be a menu whose shape says something the
    /// reader has to work out, and a greyed "Open" says it where it was always
    /// read.
    fn off_when(self, off: bool) -> Self {
        Self {
            disabled: off,
            ..self
        }
    }

    /// The destructive tone, for the rows that take something away.
    fn danger(mut self) -> Self {
        self.tone = MenuItemTone::Danger;
        self
    }

    /// A rule above this row.
    fn ruled(mut self) -> Self {
        self.ruled = true;
        self
    }
}

/// One menu's rows, drawn.
///
/// One renderer for all four menus — a book's, a folder's, the set's and the
/// level's — because the rows were never what differed between them: what
/// differs is which rows are in the list, and the list is built where the
/// facts are. What is left here is the one thing a row of these menus has to
/// know about its neighbours: the row above it asked for a rule.
#[component]
fn EntryMenu(
    /// A heading above the rows, for a menu about a set rather than about the
    /// one thing under the pointer.
    #[prop(optional, into)]
    heading: Option<String>,
    items: Vec<MenuItemSpec>,
) -> impl IntoView {
    view! {
        <>
            {heading.map(|text| view! { <SectionLabel text=text /> })}
            {items
                .into_iter()
                .map(|item| {
                    let MenuItemSpec {
                        icon,
                        label,
                        run,
                        sublabel,
                        title,
                        disabled,
                        tone,
                        ruled,
                    } = item;
                    // A second line is a different ROW shape, not a longer one
                    // (see `crate::components::primitives::menu::menu_item`),
                    // and the row's own props take a `String` rather than an
                    // `Option`: so the two cases build two rows, and a row with
                    // no tooltip passes the empty one, which is no tooltip at
                    // all and what it had before.
                    let row = match sublabel {
                        Some(note) => view! {
                            <MenuItem
                                icon=icon
                                label=label
                                sublabel=note
                                title=title.unwrap_or_default()
                                disabled=disabled
                                tone=tone
                                on_click=move || run.run(())
                            />
                        }
                            .into_any(),
                        None => view! {
                            <MenuItem
                                icon=icon
                                label=label
                                title=title.unwrap_or_default()
                                disabled=disabled
                                tone=tone
                                on_click=move || run.run(())
                            />
                        }
                            .into_any(),
                    };
                    view! {
                        {ruled.then(|| view! { <Separator spacing="my-1" /> })}
                        {row}
                    }
                })
                .collect_view()}
        </>
    }
}

/// Which kind of entry a right-click landed on.
///
/// Three of the shared rows do something different for the two kinds, and the
/// difference is a word or a service call rather than a shape — so it belongs
/// here, where the rows are built, rather than in two menus that would each
/// keep their own copy of the order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    /// A row: a book in the grid or the list, or a pointer at one.
    Row,
    /// A shelf, drawn as a folder.
    Shelf,
}

/// The rows every entry menu starts with, in the order every one of them shows
/// them: what the entry is, what to do with it, and where it came from.
///
/// The five are the same five for a book, a link and a folder, which is why
/// they are built here: two menus holding one list is how the two drifted into
/// two orders, two disabled rules and two removal rows for one gesture.
/// `dead` stands down the rows that cannot answer — a book whose address died
/// can still be renamed, relinked and removed — and the kind decides which
/// service answers `id`, never which rows the reader is offered.
fn base_entry_items(
    state: AppState,
    id: &str,
    kind: EntryKind,
    dead: bool,
    rename_sheet: RenameSheet,
    close: Callback<()>,
) -> Vec<MenuItemSpec> {
    let mut items = Vec::with_capacity(6);

    // Open. A folder opens as the page's shelf — the same write a tap makes —
    // and a row opens through the document service, which is where "a book, or
    // a pointer at one" is decided.
    let open_id = id.to_string();
    let open = match kind {
        EntryKind::Row => MenuItemSpec::new(
            IconName::Open,
            "Open",
            Callback::new(move |_| {
                close.run(());
                document::open_row(state, open_id.clone());
            }),
        )
        .off_when(dead),
        EntryKind::Shelf => MenuItemSpec::new(
            IconName::Open,
            "Open shelf",
            Callback::new(move |_| {
                close.run(());
                state.library.shelf.set(open_id.clone());
            }),
        ),
    };
    items.push(open);

    // Select. One gesture for both kinds: a right-click says "this one", and a
    // set that grows from there is the same set the check marks draw.
    let select_id = id.to_string();
    items.push(MenuItemSpec::new(
        IconName::Check,
        "Select",
        Callback::new(move |_| {
            close.run(());
            enter_selection(state, &select_id);
        }),
    ));

    // Rename. The sheet renames what the shelf SHOWS — a book's title, a
    // link's own name, a shelf's name — which is a fact every kind has,
    // address or no address: a book whose file died is exactly the book a
    // reader may want to rename before hunting the file down.
    let rename_id = id.to_string();
    let rename_title = match kind {
        EntryKind::Row => "The name the library shows — the file on disk keeps its own",
        EntryKind::Shelf => "The name the library shows — a folder on disk keeps its own",
    };
    items.push(
        MenuItemSpec::new(
            IconName::Pencil,
            "Rename…",
            Callback::new(move |_| {
                close.run(());
                match kind {
                    EntryKind::Row => rename_sheet.ask_row(state, &rename_id),
                    EntryKind::Shelf => rename_sheet.ask_shelf(state, &rename_id),
                }
            }),
        )
        .title(rename_title),
    );

    // Duplicate. A row duplicates as a row and a shelf as a shelf — a second
    // card beside the original in both cases, and for a shelf it is the
    // reader's own copy rather than a second door to one directory
    // (`crate::services::library::duplicate`).
    let dup_id = id.to_string();
    let duplicate = match kind {
        EntryKind::Row => MenuItemSpec::new(
            IconName::Copy,
            "Duplicate",
            Callback::new(move |_| {
                close.run(());
                duplicate_row(state, &dup_id);
            }),
        )
        .off_when(dead)
        .title("A second copy of this book, filed beside it"),
        EntryKind::Shelf => MenuItemSpec::new(
            IconName::Copy,
            "Duplicate",
            Callback::new(move |_| {
                close.run(());
                duplicate_shelf(state, &dup_id);
            }),
        )
        .title("A second shelf of your own, holding the same books"),
    };
    items.push(duplicate);

    // Reveal. The file manager's own view of what is behind the entry: the
    // store's copy for a book the library copied, the address itself for one
    // read at its place, and the target for a link. An entry with nothing
    // behind it gets no row at all rather than a disabled one — the menu is
    // built per ask, so the answer cannot have gone stale, and a door the
    // reader keeps trying is worse than a door that is not there.
    let path = match kind {
        EntryKind::Row => path_of_row(state, id),
        EntryKind::Shelf => path_of_shelf(state, id),
    };
    if let Some(path) = path {
        items.push(
            MenuItemSpec::new(
                IconName::Folder,
                "Reveal in folder",
                Callback::new(move |_| {
                    close.run(());
                    reveal_in_folder(state, path.clone());
                }),
            )
            .off_when(dead),
        );
    }

    items
}

/// A book's menu — a card or a row, a book or a link.
///
/// No row carries a sublabel: a right-click is a reader who knows what the rows
/// mean, and a menu that explains itself on every line is one that has to be
/// read before it can be used. What the kind adds to the shared five is the row
/// a dead address owns and the removal.
#[component]
fn BookMenu(
    state: AppState,
    id: String,
    missing: bool,
    rename_sheet: RenameSheet,
    close: Callback<()>,
) -> impl IntoView {
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");
    let mut items = base_entry_items(state, &id, EntryKind::Row, missing, rename_sheet, close);
    // The one row a dead address owns: the file the library recorded is not
    // there any more, so the way back is to point at it again.
    if missing {
        let find_id = id.clone();
        items.push(MenuItemSpec::new(
            IconName::Search,
            "Find again…",
            Callback::new(move |_| {
                close.run(());
                relink_dialog(state, find_id.clone());
            }),
        ));
    }
    items.push(
        MenuItemSpec::new(
            IconName::Close,
            "Remove from library",
            Callback::new(move |_| {
                close.run(());
                remove_sheet.ask(&id);
            }),
        )
        .danger()
        .ruled(),
    );

    view! { <EntryMenu items=items /> }
}

/// A shelf's menu, drawn as a folder.
///
/// The one place a shelf can be taken apart from without selecting it first, which
/// is the affordance the breadcrumb's parked popover used to be the only one for —
/// and the one place a shelf is subdivided from where it stands: the new shelf is
/// filed inside the one that was asked, whichever level the page is on, because
/// "new shelf" on a folder is an answer about that folder and not about the page.
///
/// It is also the one place a folder's WATCH is turned by hand, and the row is
/// the SEAT's rather than the tree's: tracking is a tree
/// (`library_core::tracking`), so a right-click on a rung shelf turns that rung —
/// an explicit decision at that rung, with the tree above keeping its own — and
/// a right-click on the root shelf turns the whole tree, which is the root's
/// seat. The import sheet's control asks the same per-rung question of the
/// ground being imported; this row is the answer for a shelf that already
/// stands (`crate::services::library::set_shelf_watch`).
#[component]
fn FolderMenu(
    state: AppState,
    id: String,
    rename_sheet: RenameSheet,
    close: Callback<()>,
) -> impl IntoView {
    let mut items = base_entry_items(state, &id, EntryKind::Shelf, false, rename_sheet, close);
    // The watch this shelf's seat answers for, when it answers for one: a
    // shelf of a folder the library reads in place, at its root or at any rung
    // of its tree, and a shelf a hand made inside one. The decision is the
    // SEAT's — the rung this shelf stands on — so the row toggles the ground
    // the reader is looking at, and the sublabel says which ground that is:
    // the whole folder at the root, only the subfolder at a rung.
    if let Some(watch) = shelf_watch(state, &id) {
        let on = watch.on;
        // The row toggles the SEAT this shelf stands on — the whole tree from
        // the watched root, and only a subfolder and the ground inside it from
        // a rung. The second line is what says which: from three shelves deep,
        // "stop watching" without a named ground is a surprise rather than a
        // toggle.
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
        };
        let watch_id = id.clone();
        items.push(
            MenuItemSpec::new(
                icon,
                label,
                Callback::new(move |_| {
                    close.run(());
                    set_shelf_watch(state, &watch_id, !on);
                }),
            )
            .sublabel(sublabel)
            .title(title),
        );
    }
    let inside_id = id.clone();
    items.push(MenuItemSpec::new(
        IconName::Plus,
        "New shelf",
        Callback::new(move |_| {
            close.run(());
            create_shelf_and_enter(state, Some(&inside_id));
        }),
    ));
    items.push(
        MenuItemSpec::new(
            IconName::Close,
            "Take shelf apart",
            Callback::new(move |_| {
                close.run(());
                delete_shelf(state, &id);
            }),
        )
        .danger()
        .ruled(),
    );

    view! { <EntryMenu items=items /> }
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
    let mut items = Vec::with_capacity(4);
    items.push(MenuItemSpec::new(
        IconName::Plus,
        "New shelf from these",
        Callback::new(move |_| {
            close.run(());
            file_selection_on_new_shelf(state);
        }),
    ));
    // One act per selected thing, and each is the single duplicate's own — a
    // copy beside the original, in the counter name the level gives it. The set
    // holds both kinds of thing a right-click lands on, so a shelf in it
    // duplicates as a shelf (its own copy, its own subtree) and a book as a
    // book; things that cannot be copied (a book whose file died) are skipped
    // by the service, which is where that fact lives.
    items.push(MenuItemSpec::new(
        IconName::Copy,
        format!("Duplicate ({count})"),
        Callback::new(move |_| {
            close.run(());
            let ids: Vec<String> = state
                .library
                .selected
                .with_untracked(|set| set.iter().cloned().collect());
            duplicate_rows(state, &ids);
        }),
    ));
    items.push(
        MenuItemSpec::new(
            IconName::Close,
            format!("Remove ({count})"),
            Callback::new(move |_| {
                close.run(());
                ask_remove_selection(state, &remove_sheet);
            }),
        )
        .danger()
        .ruled(),
    );
    items.push(MenuItemSpec::new(
        IconName::Undo,
        "Clear selection",
        Callback::new(move |_| {
            close.run(());
            exit_selection(state);
        }),
    ));

    view! { <EntryMenu heading=heading items=items /> }
}

/// The level's menu, from a right-click on empty shelf.
///
/// The two facts its second row is about — whether the level holds anything,
/// and whether a selection is running — are read when the menu is built: a menu
/// is asked for and drawn in the same breath, so nothing can change between the
/// right-click and the row it lands on.
#[component]
fn LevelMenu(
    state: AppState,
    order: ShelfOrder,
    folders: FolderOrder,
    close: Callback<()>,
) -> impl IntoView {
    let mut items = Vec::with_capacity(2);
    items.push(MenuItemSpec::new(
        IconName::Plus,
        "New shelf",
        Callback::new(move |_| {
            close.run(());
            create_shelf_and_enter(state, None);
        }),
    ));
    // A row that silently does nothing is worse than no row, so "Select all" is
    // only here while there is something on the level to select.
    let anything = !order.0.with_untracked(|books| books.is_empty())
        || !folders.0.with_untracked(|each| each.is_empty());
    if anything {
        if state.library.selecting.with_untracked(|on| *on) {
            items.push(MenuItemSpec::new(
                IconName::Undo,
                "Clear selection",
                Callback::new(move |_| {
                    close.run(());
                    exit_selection(state);
                }),
            ));
        } else {
            items.push(MenuItemSpec::new(
                IconName::Check,
                "Select all",
                Callback::new(move |_| {
                    close.run(());
                    select_on_screen(state, order, folders);
                }),
            ));
        }
    }

    view! { <EntryMenu items=items /> }
}
