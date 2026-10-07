//! The shelf's right-click: one menu per kind of thing under the pointer.

use leptos::prelude::*;

use app_chrome::icon::IconName;

use crate::features::library::content::{FolderOrder, ShelfOrder};
use crate::features::library::remove_modal::RemoveSheet;
use crate::features::library::rename_modal::RenameSheet;
use crate::features::library::selection::{
    ask_remove_selection, enter_selection, exit_selection, file_selection_on_new_shelf,
    select_on_screen,
};
use crate::services::open;
use crate::services::{
    ask_relink, ask_shelf_apart, create_shelf_and_enter, duplicate_entries, duplicate_row,
    duplicate_shelf, path_of_row, path_of_shelf, reveal_in_folder, set_shelf_watch, shelf_watch,
};
use app_ui::components::primitives::floating::context_menu::ContextMenu;
use app_ui::components::primitives::menu::menu_item::{MenuItem, MenuItemTone};
use app_ui::components::primitives::menu::section_label::SectionLabel;
use app_ui::components::primitives::menu::separator::Separator;

/// The facts, not an id: a rescan can change the list between click and row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTarget {
    /// The id alone says which of two rows of one name was meant.
    Book {
        id: String,
        missing: bool,
    },
    Folder {
        id: String,
    },
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
    /// One signal for the page: a second ask replaces the first.
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
pub(crate) fn LibraryContextMenu(state: crate::context::LibraryContext) -> impl IntoView {
    let menu = use_context::<LibraryMenuHost>().expect("the library page provides the menu");
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");
    let rename_sheet = use_context::<RenameSheet>().expect("the library page provides the sheet");
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let folders = use_context::<FolderOrder>().expect("the library content provides the folders");
    let request = menu.request;
    // Handed to every row: one acting without closing would float over the
    // shelf it changed.
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

/// The same rows in different orders; one builder writes the order and rules.
struct MenuItemSpec {
    icon: IconName,
    label: String,
    run: Callback<()>,
    sublabel: Option<String>,
    title: Option<String>,
    disabled: bool,
    tone: MenuItemTone,
    /// Marks the row that takes something away, unlike the changing rows.
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

    /// The row stays: a shape that changed says what the reader must work out.
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

/// A book's, a folder's, the set's and the level's: rows built where the
/// facts are.
#[component]
fn EntryMenu(
    #[prop(optional, into)] heading: Option<String>,
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
                    // A second line is another row shape; no tooltip
                    // passes an empty one.
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

/// A word apart, so the order lives here rather than in two menus.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Row,
    Shelf,
}

/// The rows a book, a link and a folder share: one order, one rule.
fn base_entry_items(
    state: crate::context::LibraryContext,
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
                open::open_row(&state, open_id.clone());
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

    // The sheet renames what the shelf shows, for every kind, dead included.
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
        .title(
            "A second copy of this book, the library's own — highlights and all — filed beside it",
        ),
        EntryKind::Shelf => MenuItemSpec::new(
            IconName::Copy,
            "Duplicate",
            Callback::new(move |_| {
                close.run(());
                duplicate_shelf(state, &dup_id);
            }),
        )
        .title("A second shelf of your own, holding fresh copies of its books"),
    };
    items.push(duplicate);

    // An entry with nothing behind it gets no row: the menu is built per ask.
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
    state: crate::context::LibraryContext,
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
                // Through the guarded door: one function picks dialog or sheet.
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

/// The one place a shelf is taken apart, and subdivided where it stands.
#[component]
fn FolderMenu(
    state: crate::context::LibraryContext,
    id: String,
    rename_sheet: RenameSheet,
    close: Callback<()>,
) -> impl IntoView {
    let mut items = base_entry_items(state, &id, EntryKind::Shelf, false, rename_sheet, close);
    // The decision is the seat's: the row toggles the rung's own ground.
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

/// Read when the menu is built: the heading and the row cannot disagree.
#[component]
fn SelectionMenu(
    state: crate::context::LibraryContext,
    remove_sheet: RemoveSheet,
    close: Callback<()>,
) -> impl IntoView {
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

/// Its second row is read when the menu is built: nothing changes in between.
#[component]
fn LevelMenu(
    state: crate::context::LibraryContext,
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
