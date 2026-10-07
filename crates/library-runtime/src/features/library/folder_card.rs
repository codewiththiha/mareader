//! A shelf on the page, drawn as a folder: a 2x2 plate of its contents.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::Book;
use library_core::shelf::{Shelf, children_of, find};
use library_core::text::plural;

use crate::features::library::entry::{EntryDescriptor, EntryShell};
use crate::features::library::folder_badge::FolderBadge;
use crate::features::library::gestures::folder_policy;
use crate::features::library::selection::SelectionCheck;
use crate::features::library::shelf_item::SeamVocab;

/// Two by two, always four cells: one book is one cover and three
/// hatched quarters.
pub(crate) const THUMB_CAP: usize = 4;

const PLATE_DEPTH: usize = 2;

#[component]
pub(crate) fn FolderCard(state: crate::context::LibraryContext, shelf: Shelf) -> impl IntoView {
    // Identity only: a keyed row survives, so movable facts are read by id.
    let id = shelf.id.clone();

    let name = state.library.shelf_name_signal(&id);

    let count_id = id.clone();
    let counts = Signal::derive(move || {
        let books = state.library.shelves.with(|shelves| {
            find(shelves, &count_id)
                .map(|s| s.books.len())
                .unwrap_or_default()
        });
        let inside = state
            .library
            .shelves
            .with(|shelves| children_of(shelves, Some(count_id.as_str())).len());
        (books, inside)
    });

    // The rung's own tracking question, not the whole import's.
    let dot_id = id.clone();
    let watched = Signal::derive(move || state.library.shelf_tracked(&dot_id));

    let badge_id = id.clone();

    let check_id = id.clone();

    // Open drills the route; right-click asks about the folder.
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
                <FolderBadge state=state shelf_id=badge_id class="folder-mode" />
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

/// One cell of a plate: a folder's own plate, or a book's cover.
#[derive(Clone)]
enum PlateItem {
    Folder(String),
    Book(Book),
}

/// Folders first, then books, in the page's own order.
fn plate_items(state: crate::context::LibraryContext, shelf_id: &str) -> Vec<PlateItem> {
    let (folders, members): (Vec<String>, Vec<String>) = state.library.shelves.with(|shelves| {
        (
            children_of(shelves, Some(shelf_id))
                .iter()
                .map(|s| s.id.clone())
                .collect(),
            find(shelves, shelf_id)
                .map(|s| s.books.clone())
                .unwrap_or_default(),
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

/// Recursive: "what is inside" is the same question at every depth.
#[component]
fn Plate(state: crate::context::LibraryContext, shelf_id: String, depth: usize) -> impl IntoView {
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
fn CoverCell(state: crate::context::LibraryContext, book: Book) -> impl IntoView {
    let path = book.path().to_string();
    let empty_path = path.clone();
    let alt = book.title();
    // Read at the build: the items refire with the books list.
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

/// Counts both halves; shared with the list's tree rows.
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
