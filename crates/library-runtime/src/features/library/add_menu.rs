//! The ways books arrive: the picker, a folder, or a restore.

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use app_chrome::icon::{Icon, IconName};
use library_core::ledger::{Recovered, index_by_fp, recoverables};
use library_core::shelf::{ALL_SHELF, find};
use library_core::text::{human_age, human_size};

use crate::features::library::import_modal::ImportSheet;
use crate::services::{
    folder_label, import_files, pick_documents, pick_documents_in, restore_deleted_book,
};
use app_ui::components::primitives::controls::button::{Button, ButtonVariant};
use app_ui::components::primitives::floating::menu_popover::MenuPopover;
use app_ui::components::primitives::menu::menu_item::MenuItem;
use app_ui::components::primitives::menu::section_label::SectionLabel;
use app_ui::components::primitives::menu::separator::Separator;

#[derive(Debug, Clone, PartialEq)]
struct RestoreRow {
    item: Recovered,
    /// Still listed but disabled: dropping rows would read as loss.
    gone: bool,
}

impl RestoreRow {
    fn label(&self) -> String {
        match &self.item {
            Recovered::Deleted(entry) => entry.label(),
            Recovered::Moved { title, path, .. } => {
                library_core::text::display_or_stem(title.as_deref(), path)
            }
        }
    }

    fn sublabel(&self, now_ms: u64) -> String {
        if self.gone {
            return "not there any more".to_string();
        }
        match &self.item {
            Recovered::Deleted(entry) => {
                let age = human_age(entry.removed_ms, now_ms);
                match entry.fp.mtime_ms {
                    0 => format!("removed {age}"),
                    _ => format!("removed {age} · {}", human_size(entry.fp.size)),
                }
            }
            Recovered::Moved { home_shelf, .. } => match home_shelf {
                Some(name) => format!("now on “{name}”"),
                None => "in the library, on no shelf".to_string(),
            },
        }
    }

    fn icon(&self) -> IconName {
        match self.item {
            Recovered::Deleted(_) => IconName::Undo,
            Recovered::Moved { .. } => IconName::Next,
        }
    }

    /// A restore is explicit, so it says it ignores the folder's filters.
    fn hint(&self) -> &'static str {
        match self.item {
            Recovered::Deleted(_) => {
                "Add this file back, even if it doesn't match the folder's filters"
            }
            Recovered::Moved { .. } => "This book moved to another shelf",
        }
    }
}

fn candidates(state: crate::context::LibraryContext, folder_id: &str) -> Vec<RestoreRow> {
    let Some(folder) = state.library.folder(folder_id) else {
        return Vec::new();
    };
    let rows = state.library.books.get_untracked();
    let shelves = state.library.shelves.get_untracked();
    let index = index_by_fp(&rows);
    recoverables(&folder, &index, &shelves)
        .into_iter()
        .map(|item| RestoreRow { item, gone: false })
        .collect()
}

/// `root` narrows the picker to one watched folder; `None` is the whole disk.
fn from_files(
    state: crate::context::LibraryContext,
    target: Option<String>,
    root: Option<String>,
) {
    spawn_local(async move {
        let picked = match root {
            Some(root) => pick_documents_in(root).await,
            None => pick_documents().await,
        };
        // The dialog outlives the library: a disposed state would abort.
        if state.library.books.try_get_untracked().is_none() {
            return;
        }
        match picked {
            Ok(paths) if paths.is_empty() => {}
            Ok(paths) => import_files(state, paths, target),
            Err(message) => crate::services::toast(state, message),
        }
    });
}

/// A folder has options, so it gets the import sheet.
fn from_directory(sheet: ImportSheet) {
    spawn_local(async move {
        let picked = crate::services::pick_folder().await;
        // The dialog outlives the library it was opened from.
        if sheet.open.try_get_untracked().is_none() {
            return;
        }
        match picked {
            Ok(Some(root)) => sheet.open_on(Some(root)),
            Ok(None) => {}
            Err(message) => sheet.toast(message),
        }
    });
}

/// At the root nothing is filed: "All" is an order, not a shelf.
fn add_target(state: crate::context::LibraryContext) -> Signal<Option<String>> {
    Signal::derive(move || {
        let id = state.library.shelf.get();
        (id != ALL_SHELF).then_some(id)
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AddFace {
    /// The shelf's own add card; a grid with a hole reads as unfinished.
    Card,
    Row,
    Empty,
}

#[component]
pub(crate) fn AddMenuButton(state: crate::context::LibraryContext, face: AddFace) -> impl IntoView {
    let open = RwSignal::new(false);
    let anchor: NodeRef<html::Div> = NodeRef::new();
    let target = match face {
        AddFace::Empty => Signal::derive(|| None),
        AddFace::Card | AddFace::Row => add_target(state),
    };
    let wrapper = match face {
        AddFace::Card => "book-card book-add",
        AddFace::Row => "relative",
        AddFace::Empty => "relative flex max-w-md flex-col items-center gap-4 text-center",
    };
    let has_tauri = tauri_bridge::has_tauri();

    view! {
        <div node_ref=anchor class=wrapper>
            {match face {
                AddFace::Card => {
                    view! {
                        <button
                            class="book-cover book-add-cover"
                            type="button"
                            aria-label="Add books"
                            aria-haspopup="menu"
                            aria-expanded=move || open.get().to_string()
                            title="Add books"
                            on:click=move |_| open.set(!open.get_untracked())
                        >
                            <Icon name=IconName::Plus size=32 class="text-muted" />
                        </button>
                    }
                        .into_any()
                }
                AddFace::Row => {
                    view! {
                        <button
                            type="button"
                            aria-label="Add books"
                            aria-haspopup="menu"
                            aria-expanded=move || open.get().to_string()
                            title="Add books"
                            on:click=move |_| open.set(!open.get_untracked())
                            class="lib-add-row"
                        >
                            <Icon name=IconName::Plus size=15 />
                            <span>"Add books"</span>
                        </button>
                    }
                        .into_any()
                }
                AddFace::Empty => {
                    view! {
                        <>
                            <Button
                                on_click=move |_| open.set(!open.get_untracked())
                                variant=ButtonVariant::Primary
                                active=Signal::derive(move || open.get())
                                title="Import books"
                            >
                                <Icon name=IconName::Plus size=17 />
                                <span>"Import books"</span>
                            </Button>
                            {has_tauri
                                .then(|| {
                                    view! {
                                        <p class="text-xs text-muted">
                                            {format!(
                                                "Or drop a {} file anywhere in the window",
                                                reader_core::format::kind_list()
                                            )}
                                        </p>
                                    }
                                })}
                        </>
                    }
                        .into_any()
                }
            }}
            <AddMenu state=state open=open anchor=anchor target=target />
        </div>
    }
}

#[component]
fn AddMenu(
    state: crate::context::LibraryContext,
    open: RwSignal<bool>,
    anchor: NodeRef<html::Div>,
    target: Signal<Option<String>>,
) -> impl IntoView {
    let sheet = use_context::<ImportSheet>().expect("the library page provides the import sheet");

    let folder_id = Signal::derive(move || {
        let id = state.library.shelf.get();
        if id == ALL_SHELF {
            return None;
        }
        state
            .library
            .shelves
            .with(|shelves| find(shelves, &id).and_then(|s| s.kind.folder_id().map(str::to_string)))
    });

    let rows = RwSignal::new(Vec::<RestoreRow>::new());
    // Two different answers, so the list swaps for a confirm in place.
    let confirm = RwSignal::new(None::<Recovered>);

    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        confirm.set(None);
        let Some(folder_id) = folder_id.get() else {
            rows.set(Vec::new());
            return;
        };
        let built = candidates(state, &folder_id);
        rows.set(built.clone());
        let paths: Vec<String> = built.iter().filter_map(deleted_path).collect();
        if paths.is_empty() {
            return;
        }
        spawn_local(async move {
            let Ok(checks) = crate::services::verify_paths(paths).await else {
                return;
            };
            rows.update(|rows| {
                for check in &checks {
                    if let Some(row) = rows
                        .iter_mut()
                        .find(|r| deleted_path(r).is_some_and(|p| p == check.path))
                    {
                        row.gone = !check.exists;
                    }
                }
            });
        });
    });

    let has_rows = Signal::derive(move || rows.with(|r| !r.is_empty()));

    view! {
        <MenuPopover
            open=open
            anchor=anchor
            width=264u32
            class="max-h-80 overflow-y-auto p-1".to_string()
        >
            {move || {
                if let Some(item) = confirm.get() {
                    return view! {
                        <Confirm
                            state=state
                            open=open
                            confirm=confirm
                            item=item
                            target=target
                        />
                    }
                        .into_any();
                }
                view! {
                    <>
                        <MenuItem
                            icon=IconName::Open
                            label="Choose files…"
                            on_click=move || {
                                open.set(false);
                                from_files(state, target.get_untracked(), None);
                            }
                        />
                        <MenuItem
                            icon=IconName::Library
                            label="Choose a folder…"
                            on_click=move || {
                                open.set(false);
                                from_directory(sheet);
                            }
                        />
                        {move || {
                            folder_id.get().and_then(|id| state.library.folder(&id)).map(|folder| {
                                let root = folder.root.clone();
                                view! {
                                    <>
                                        <Separator spacing="my-1" />
                                        <MenuItem
                                            icon=IconName::Drop
                                            label="Choose files from this folder"
                                            sublabel=folder_label(&root)
                                            on_click=move || {
                                                open.set(false);
                                                from_files(
                                                    state,
                                                    target.get_untracked(),
                                                    Some(root.clone()),
                                                );
                                            }
                                        />
                                    </>
                                }
                            })
                        }}
                        {move || {
                            has_rows.get().then(|| {
                                view! {
                                    <>
                                        <Separator spacing="my-1" />
                                        <SectionLabel text="Restore" />
                                        {move || {
                                            let now = runtime_contract::time::now_ms();
                                            rows.get()
                                                .into_iter()
                                                .map(|row| {
                                                    view! {
                                                        <RestoreItem
                                                            state=state
                                                            open=open
                                                            confirm=confirm
                                                            row=row
                                                            now=now
                                                        />
                                                    }
                                                })
                                                .collect_view()
                                        }}
                                    </>
                                }
                            })
                        }}
                    </>
                }
                    .into_any()
            }}
        </MenuPopover>
    }
}

fn deleted_path(row: &RestoreRow) -> Option<String> {
    match &row.item {
        Recovered::Deleted(entry) => Some(entry.last_path.clone()),
        Recovered::Moved { .. } => None,
    }
}

/// Read at click time: the menu outlives the render that built it.
fn current_folder_id(state: crate::context::LibraryContext) -> Option<String> {
    let shelf_id = state.library.shelf.get_untracked();
    if shelf_id == ALL_SHELF {
        return None;
    }
    state.library.shelf_folder_id(&shelf_id)
}

/// Its own component: a row is four strings and a branch.
#[component]
fn RestoreItem(
    state: crate::context::LibraryContext,
    open: RwSignal<bool>,
    confirm: RwSignal<Option<Recovered>>,
    row: RestoreRow,
    now: u64,
) -> impl IntoView {
    let label = row.label();
    let sublabel = row.sublabel(now);
    let hint = row.hint().to_string();
    let gone = row.gone;
    let icon = row.icon();
    let item = row.item;

    view! {
        <MenuItem
            icon=icon
            label=label
            sublabel=sublabel
            title=hint
            disabled=gone
            on_click=move || {
                match item.clone() {
                    Recovered::Deleted(entry) => {
                        let Some(folder_id) = current_folder_id(state) else {
                            return;
                        };
                        open.set(false);
                        restore_deleted_book(state, folder_id, entry.fp);
                    }
                    Recovered::Moved { .. } => confirm.set(Some(item.clone())),
                }
            }
        />
    }
}

#[component]
fn Confirm(
    state: crate::context::LibraryContext,
    open: RwSignal<bool>,
    confirm: RwSignal<Option<Recovered>>,
    item: Recovered,
    target: Signal<Option<String>>,
) -> impl IntoView {
    let (book_id, title, home) = match &item {
        Recovered::Moved {
            book_id,
            title,
            home_shelf,
            ..
        } => (book_id.clone(), title.clone(), home_shelf.clone()),
        Recovered::Deleted(_) => (String::new(), None, None),
    };
    let label = title.clone().unwrap_or_else(|| "This book".to_string());
    let go_label = match &home {
        Some(name) => format!("Show it in {name}"),
        None => "Show it in Home".to_string(),
    };
    let here_id = book_id.clone();
    let go_id = book_id;
    let question = format!("“{label}” is on another shelf.");

    view! {
        <>
            <MenuItem
                icon=IconName::Prev
                label="Back"
                on_click=move || confirm.set(None)
            />
            <Separator spacing="my-1" />
            <p class="px-2 py-1.5 text-xs text-muted">{question}</p>
            <MenuItem
                icon=IconName::Plus
                label="Also show it here"
                sublabel="One book, two shelves — nothing is copied".to_string()
                on_click=move || {
                    open.set(false);
                    if let Some(shelf_id) = target.get_untracked() {
                        crate::services::also_show(state, &here_id, &shelf_id);
                    }
                }
            />
            <MenuItem
                icon=IconName::Next
                label=go_label
                sublabel="Closes this menu and takes you to it".to_string()
                on_click=move || {
                    open.set(false);
                    crate::services::reveal_book(state, &go_id);
                }
            />
        </>
    }
}

#[cfg(test)]
mod tests {
    use super::RestoreRow;
    use library_core::book::Fingerprint;
    use library_core::folder::Tombstone;
    use library_core::ledger::Recovered;
    use reader_core::format::Format;

    const MINUTE: u64 = 60_000;
    const NOW: u64 = 1_700_000_000_000;

    fn fp(size: u64) -> Fingerprint {
        Fingerprint {
            size,
            mtime_ms: 1,
            head_hash: 1,
        }
    }

    fn removed(title: Option<&str>, path: &str, size: u64, ago_ms: u64) -> RestoreRow {
        RestoreRow {
            item: Recovered::Deleted(Tombstone {
                fp: fp(size),
                title: title.map(str::to_string),
                format: Format::Pdf,
                last_path: path.to_string(),
                shelf_id: None,
                removed_ms: NOW - ago_ms,
                moved: false,
                returned_row: None,
            }),
            gone: false,
        }
    }

    #[test]
    fn a_removed_book_is_named_by_its_title_or_by_its_file() {
        assert_eq!(
            removed(Some("Dune"), "/books/dune.pdf", 1, 0).label(),
            "Dune"
        );
        assert_eq!(
            removed(None, "/books/rust-book.pdf", 1, 0).label(),
            "rust-book"
        );
    }

    #[test]
    fn a_removed_book_says_how_long_ago_and_how_big() {
        let row = removed(
            Some("Dune"),
            "/books/dune.pdf",
            12 * 1024 * 1024,
            3 * MINUTE,
        );
        assert_eq!(row.sublabel(NOW), "removed 3 minutes ago · 12 MB");
    }

    #[test]
    fn a_book_never_measured_shows_no_size_it_does_not_have() {
        let row = RestoreRow {
            item: Recovered::Deleted(Tombstone {
                fp: Fingerprint {
                    size: 18,
                    mtime_ms: 0,
                    head_hash: 1,
                },
                title: Some("Dune".into()),
                format: Format::Pdf,
                last_path: "/books/dune.pdf".into(),
                shelf_id: None,
                removed_ms: NOW - MINUTE,
                moved: false,
                returned_row: None,
            }),
            gone: false,
        };
        assert_eq!(row.sublabel(NOW), "removed 1 minute ago");
    }

    #[test]
    fn a_row_whose_file_is_gone_says_so_instead_of_vanishing() {
        let row = RestoreRow {
            gone: true,
            ..removed(Some("Dune"), "/books/dune.pdf", 1, MINUTE)
        };
        assert_eq!(row.sublabel(NOW), "not there any more");
    }

    #[test]
    fn a_moved_book_names_the_shelf_it_went_to() {
        let row = RestoreRow {
            item: Recovered::Moved {
                book_id: "b1".into(),
                title: Some("Dune".into()),
                path: "/books/dune.pdf".into(),
                home_shelf: Some("Fiction".into()),
            },
            gone: false,
        };
        assert_eq!(row.label(), "Dune");
        assert_eq!(row.sublabel(NOW), "now on “Fiction”");
        let homeless = RestoreRow {
            item: Recovered::Moved {
                book_id: "b1".into(),
                title: Some("Dune".into()),
                path: "/books/dune.pdf".into(),
                home_shelf: None,
            },
            gone: false,
        };
        assert_eq!(homeless.sublabel(NOW), "in the library, on no shelf");
    }

    #[test]
    fn a_restore_says_that_it_ignores_the_folders_filters() {
        let row = removed(Some("Dune"), "/books/dune.pdf", 1, 0);
        assert!(row.hint().contains("filters"));
    }
}
