//! The shelf's right-click: one menu per kind of thing under the pointer.
//!
//! A card used to answer a right-click with the removal receipt and nothing else, which is one
//! row of a menu wearing the whole gesture.

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
    ask_shelf_apart, create_shelf_and_enter, duplicate_entries, duplicate_row, duplicate_shelf,
    ask_relink, path_of_row, path_of_shelf, reveal_in_folder, set_shelf_watch, shelf_watch,
};
use crate::state::AppState;

/// Carrying the facts rather than an id: a menu row that asked the library what it was pointing at would be reading a list a rescan can change between the click and the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTarget {
    /// The id is all an open needs — it is what says WHICH row the reader meant when the library holds two of one name — so carrying the address beside it would be a second answer.
    Book { id: String, missing: bool },
    Folder { id: String },
    Selection,
    Level,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MenuRequest {
    pub x: f64,
    pub y: f64,
    pub target: MenuTarget,
}

#[derive(Clone, Copy)]
pub struct LibraryMenuHost {
    /// One signal for the whole page, so exactly one menu is up at a time and asking for a second closes the first by replacing it.
    pub request: RwSignal<Option<MenuRequest>>,
}

impl LibraryMenuHost {
    pub fn provide() -> Self {
        let this = Self {
            request: RwSignal::new(None),
        };
        provide_context(this);
        this
    }

    pub fn ask(&self, x: f64, y: f64, target: MenuTarget) {
        self.request.set(Some(MenuRequest { x, y, target }));
    }
}

#[component]
pub(crate) fn LibraryContextMenu(state: AppState) -> impl IntoView {
    let menu = use_context::<LibraryMenuHost>().expect("the library page provides the menu");
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");
    let rename_sheet = use_context::<RenameSheet>().expect("the library page provides the sheet");
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let folders = use_context::<FolderOrder>().expect("the library content provides the folders");
    let request = menu.request;
    // Handed to whichever row was chosen rather than left to each menu to remember: a row that acted without closing would leave a menu floating over the shelf it had just changed.
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

/// The rows of the four menus here are the same rows in different orders — open, select, rename,
/// duplicate, reveal, and then whatever the thing under the pointer owns — and a `<MenuItem>`
/// written per menu meant the order, the disabled rule and the removal's own rule were each
/// repeated.
struct MenuItemSpec {
    icon: IconName,
    label: String,
    run: Callback<()>,
    sublabel: Option<String>,
    title: Option<String>,
    disabled: bool,
    tone: MenuItemTone,
    /// The row that takes something away is a different kind of thing from the rows that change the library, and the rule is what says so.
    ruled: bool,
}

impl MenuItemSpec {
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

    fn sublabel(mut self, text: String) -> Self {
        self.sublabel = Some(text);
        self
    }

    fn title(mut self, text: &'static str) -> Self {
        self.title = Some(text.to_string());
        self
    }

    /// The row STAYS: a menu whose rows disappeared with the card's luck would be a menu whose shape says something the reader has to work out.
    fn off_when(self, off: bool) -> Self {
        Self {
            disabled: off,
            ..self
        }
    }

    fn danger(mut self) -> Self {
        self.tone = MenuItemTone::Danger;
        self
    }

    fn ruled(mut self) -> Self {
        self.ruled = true;
        self
    }
}

/// A book's, a folder's, the set's and the level's — what differs is which rows are in the list, and the list is built where the facts are. What is left here is the rule above a row.
#[component]
fn EntryMenu(
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
                    // A second line is a different ROW shape, not a longer one (see `crate::components::primitives::menu::menu_item`), so the two cases build two rows and a row with no tooltip passes the empty one, which is no tooltip at all.
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

/// The difference is a word or a service call rather than a shape, so it belongs here rather than in two menus that would each keep their own copy of the order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Row,
    Shelf,
}

/// The five are the same five for a book, a link and a folder: two menus holding one list is how the two drifted into two orders, two disabled rules and two removal rows for one gesture.
fn base_entry_items(
    state: AppState,
    id: &str,
    kind: EntryKind,
    dead: bool,
    rename_sheet: RenameSheet,
    close: Callback<()>,
) -> Vec<MenuItemSpec> {
    let mut items = Vec::with_capacity(6);

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

    let select_id = id.to_string();
    items.push(MenuItemSpec::new(
        IconName::Check,
        "Select",
        Callback::new(move |_| {
            close.run(());
            enter_selection(state, &select_id);
        }),
    ));

    // The sheet renames what the shelf SHOWS, which every kind has, address or no address: a book whose file died is exactly the book a reader may want to rename before hunting the file down.
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

    // An entry with nothing behind it gets no row at all rather than a disabled one — the menu is built per ask, so the answer cannot have gone stale.
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
    if missing {
        let find_id = id.clone();
        items.push(MenuItemSpec::new(
            IconName::Search,
            "Find again…",
            Callback::new(move |_| {
                close.run(());
                // Through the guarded door rather than the picker's: one function decides
                // whether a reader-page open goes straight to the dialog or the sheet comes
                // up, and a second caller of the raw picker was a second answer to drift.
                ask_relink(state, find_id.clone());
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

/// The one place a shelf can be taken apart from without selecting it first, and the one place a shelf is subdivided from where it stands: "new shelf" on a folder is an answer about that folder.
#[component]
fn FolderMenu(
    state: AppState,
    id: String,
    rename_sheet: RenameSheet,
    close: Callback<()>,
) -> impl IntoView {
    let mut items = base_entry_items(state, &id, EntryKind::Shelf, false, rename_sheet, close);
    // The decision is the SEAT's — the rung this shelf stands on — so the row toggles the ground the reader is looking at.
    if let Some(watch) = shelf_watch(state, &id) {
        let on = watch.on;
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
                ask_shelf_apart(state, &id);
            }),
        )
        .danger()
        .ruled(),
    );

    view! { <EntryMenu items=items /> }
}

/// The count is read when the menu is built rather than carried in the request, so the heading and the removal row cannot disagree. It is a number and not a signal because a menu row's label is a `String` and the set cannot change while a menu is up.
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
    items.push(MenuItemSpec::new(
        IconName::Copy,
        format!("Duplicate ({count})"),
        Callback::new(move |_| {
            close.run(());
            let ids: Vec<String> = state
                .library
                .selected
                .with_untracked(|set| set.iter().cloned().collect());
            duplicate_entries(state, &ids);
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

/// The two facts its second row is about are read when the menu is built: a menu is asked for and drawn in the same breath, so nothing can change between the right-click and the row.
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
