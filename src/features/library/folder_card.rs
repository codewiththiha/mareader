//! A shelf on the page, drawn as a folder: a 2×2 plate of what is inside it — covers for its
//! books and a plate of their own for its folders, recursively — then its name and what it
//! holds.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::Book;
use library_core::shelf::{Shelf, children_of, find};
use library_core::text::plural;

use crate::features::library::entry::{EntryDescriptor, EntryShell};
use crate::features::library::gestures::folder_policy;
use crate::features::library::selection::SelectionCheck;
use crate::features::library::shelf_item::SeamVocab;
use crate::state::AppState;

/// Two by two: a folder is recognised by what is inside it, and past four cells the plate is a
/// mosaic nobody reads. Always four cells whatever the folder holds, so one book is one cover and
/// three hatched quarters rather than one big rectangle that reads as a book card.
pub(crate) const THUMB_CAP: usize = 4;

const PLATE_DEPTH: usize = 2;

#[component]
pub(crate) fn FolderCard(state: AppState, shelf: Shelf) -> impl IntoView {
    // A keyed row is not re-created when the shelf's CONTENTS change, so everything that can move is read back out of the state by id, and the prop supplies only the identity.
    let id = shelf.id.clone();

    let name = state.library.shelf_name_signal(&id);

    let count_id = id.clone();
    let counts = Signal::derive(move || {
        let books = state
            .library
            .shelves
            .with(|shelves| find(shelves, &count_id).map(|s| s.books.len()).unwrap_or_default());
        let inside = state
            .library
            .shelves
            .with(|shelves| children_of(shelves, Some(count_id.as_str())).len());
        (books, inside)
    });

    // The question is the rung's and not the whole import's, so a subfolder turned off under a watched tree stops breathing while the tree above it keeps watching.
    let dot_id = id.clone();
    let watched = Signal::derive(move || state.library.shelf_tracked(&dot_id));

    let mode_id = id.clone();
    let mode = Signal::derive(move || state.library.shelf_mode(&mode_id));

    let check_id = id.clone();

    // The folder's own answers: "open" drills the breadcrumb route, and the right-click asks about a folder. A set being selected is not a reason to refuse a drag.
    let open_id = id.clone();
    let open = Callback::new(move |_| state.library.shelf.set(open_id.clone()));
    let entry = EntryDescriptor {
        id: id.clone(),
        vocab: SeamVocab::FolderCard,
        base_class: "folder-card",
        policy: folder_policy(&id, name, open, None),
    };

    view! {
        <EntryShell state=state entry=entry>
            <div class="folder-thumb-grid">
                <Plate state=state shelf_id=id.clone() depth=0 />
                <SelectionCheck state=state id=check_id />
            </div>
            <div class="folder-meta">
                <span class="folder-name" title=move || { name.get() }>
                    {move || name.get()}
                </span>
                <span class="folder-count">{move || summary(counts.get())}</span>
            </div>
            <div class="folder-badges">
                {move || {
                    mode.get().map(|mode| {
                        let title = if mode.copies_files() {
                            "Every book here is a copy the library keeps — the folder on disk can go."
                        } else {
                            "These books stay in their folder on disk; the library only remembers \
                             where they are."
                        };
                        view! { <span class="folder-mode" title=title>{mode.badge()}</span> }
                    })
                }}
                {move || {
                    watched.get().then(|| {
                        view! {
                            <span class="folder-watched" title="Watched for new books"></span>
                        }
                    })
                }}
            </div>
        </EntryShell>
    }
}

/// What fills one cell of a plate: a folder, previewed as a plate of its own, or
/// a book, previewed as its cover.
#[derive(Clone)]
enum PlateItem {
    Folder(String),
    Book(Book),
}

/// Folders first, then books — a plate that disagreed with the page about what is inside the folder would be a preview of something else. Books and folders share the four cells rather than each having their own four.
fn plate_items(state: AppState, shelf_id: &str) -> Vec<PlateItem> {
    let (folders, members): (Vec<String>, Vec<String>) =
        state.library.shelves.with(|shelves| {
            (
                children_of(shelves, Some(shelf_id))
                    .iter()
                    .map(|s| s.id.clone())
                    .collect(),
                find(shelves, shelf_id).map(|s| s.books.clone()).unwrap_or_default(),
            )
        });
    let books: Vec<Book> = state.library.books.with(|rows| {
        members
            .iter()
            .filter_map(|member| library_core::book::find_row(rows, member))
            .filter_map(|row| row.book().cloned())
            .collect()
    });
    let mut out: Vec<PlateItem> = folders
        .into_iter()
        .map(PlateItem::Folder)
        .chain(books.into_iter().map(PlateItem::Book))
        .collect();
    out.truncate(THUMB_CAP);
    out
}

/// Recursive on purpose: a cell that holds a folder holds that folder's OWN plate, because "what is inside this folder" is the same question at every depth.
#[component]
fn Plate(state: AppState, shelf_id: String, depth: usize) -> impl IntoView {
    let items = Signal::derive(move || plate_items(state, &shelf_id));
    view! {
        {move || {
            let items = items.get();
            (0..THUMB_CAP)
                .map(|at| match items.get(at) {
                    Some(PlateItem::Folder(id)) => {
                        let id = id.clone();
                        if depth < PLATE_DEPTH {
                            view! {
                                <span class="folder-thumb-cell">
                                    <span class="folder-thumb-grid">
                                        <Plate state=state shelf_id=id depth=depth + 1 />
                                    </span>
                                </span>
                            }
                                .into_any()
                        } else {
                            view! {
                                <span
                                    class="folder-thumb-cell folder-thumb-deep"
                                    title="A folder, deeper than a preview can show"
                                >
                                    <Icon name=IconName::Open size=12 />
                                </span>
                            }
                                .into_any()
                        }
                    }
                    Some(PlateItem::Book(book)) => {
                        view! { <CoverCell state=state book=book.clone() /> }.into_any()
                    }
                    None => {
                        view! { <span class="folder-thumb-cell folder-thumb-empty"></span> }
                            .into_any()
                    }
                })
                .collect_view()
        }}
    }
}

#[component]
fn CoverCell(state: AppState, book: Book) -> impl IntoView {
    let path = book.path().to_string();
    let empty_path = path.clone();
    let alt = book.title();
    // Read at the build rather than tracked here — the plate's items signal re-fires on every change to the books list, and the cell this rebuilds is the cell that knows.
    let missing = book.missing;
    view! {
        <span
            class="folder-thumb-cell"
            class=("folder-thumb-missing", missing)
            class=("folder-thumb-empty", move || {
                state
                    .library
                    .covers
                    .with(|covers| !covers.contains_key(&empty_path))
            })
        >
            {move || {
                match state
                    .library
                    .covers
                    .with(|covers| covers.get(&path).cloned())
                {
                    Some(cover) => {
                        view! {
                            <img
                                class="folder-thumb-img"
                                src=cover.data_url.clone()
                                alt=alt.clone()
                                loading="lazy"
                                draggable="false"
                            />
                        }
                            .into_any()
                    }
                    None => ().into_any(),
                }
            }}
        </span>
    }
}

/// Both halves: "3 books" on a folder with two shelves inside it would leave out the rest of the library down that path. Shared with the list's tree rows.
pub(crate) fn summary(counts: (usize, usize)) -> String {
    let (books, inside) = counts;
    let mut parts: Vec<String> = Vec::with_capacity(2);
    if books > 0 {
        parts.push(plural(books, "book", "books"));
    }
    if inside > 0 {
        parts.push(plural(inside, "shelf", "shelves"));
    }
    if parts.is_empty() {
        "Empty".to_string()
    } else {
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_line_says_both_halves_and_neither_when_there_is_nothing() {
        assert_eq!(summary((0, 0)), "Empty");
        assert_eq!(summary((1, 0)), "1 book");
        assert_eq!(summary((3, 0)), "3 books");
        assert_eq!(summary((0, 1)), "1 shelf");
        assert_eq!(summary((0, 2)), "2 shelves");
        assert_eq!(summary((3, 1)), "3 books · 1 shelf");
        assert_eq!(summary((1, 4)), "1 book · 4 shelves");
    }
}
