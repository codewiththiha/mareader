//! A shelf on the page, drawn as a folder: a 2×2 plate of what is inside it —
//! covers for its books and a plate of their own for its folders, recursively —
//! then its name and what it holds.
//!
//! It replaces the shelf tile, which spanned the whole grid as a row of spines
//! on a board. That shape could not nest — a row is a section, and a section
//! cannot be inside another section — so a shelf inside a shelf had nowhere to
//! be drawn. A folder is a cell like a book's, which is what makes every level
//! of the library the same shape as the one above it.
//!
//! Called a folder because that is what it looks like; the domain word stays
//! *shelf*, because *folder* already means a watched directory in
//! `library_core`, and the two are different facts about the same row (see
//! [`Shelf::parent`]'s note on `ShelfKind::Folder`).
//!
//! Three gestures share the card and the shelf's one wiring decides between
//! them — `crate::features::library::gestures`, on the
//! `crate::components::primitives::interactions::draggable_item` wrapper. A tap
//! opens the shelf, a hold starts a multi-select with this shelf already in it,
//! and a movement hands the press to `crate::features::library::dnd`, which is
//! what files a book dropped on it or nests a shelf dropped on it.
//!
//! Every card drags, including one cut from a watched folder. What the drag
//! MEANS is the services' answer rather than the card's: a shelf of a copying
//! folder, and every shelf the reader owns, is filed where the hand takes it;
//! a shelf a READ-AT-PLACE folder named is the OS directory itself, and a hand
//! taking it off the seat its tree names is a departure — the sheet asks, and
//! what lands where the hand pointed is the library's own copy of the shelf
//! and of every read-at-place book standing on it
//! (`crate::services::library::arrange`). The folder on disk is untouched, and
//! the next import of it mints the original tree back and lights it up.
//!
//! A drop this folder refuses — a shelf that would end up inside itself — wears no
//! ring at all, which is the honest half of the gesture: the decision table says
//! no before the pointer gets there, so the reader is never offered a drop that
//! the commit step would then quietly decline.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::Book;
use library_core::shelf::{Shelf, children_of, find};
use library_core::text::plural;

use crate::features::library::context_menu::MenuTarget;
use crate::features::library::gestures::ShelfItemPolicy;
use crate::features::library::selection::SelectionCheck;
use crate::features::library::shelf_item::{SeamVocab, ShelfItemShell};
use crate::state::AppState;

/// How many cells a plate has, and the most it fills. Two by two: a folder is
/// recognised by what is inside it, and past four cells the plate is a mosaic
/// nobody reads — the line under the name is the answer to "how much". Always
/// four cells whatever the folder holds, so one book is one cover and three
/// hatched quarters rather than one big rectangle that reads as a book card.
///
/// Shared with the fold preview a drag draws
/// (`crate::features::library::dnd::layer`), because that preview is a promise
/// about this plate and a promise drawn with a different number of cells is a
/// promise about a folder the library does not have.
pub(crate) const THUMB_CAP: usize = 4;

/// The deepest plate the preview recurses to: the folder's own plate, the plates
/// of the folders inside it, and the plates of the folders inside those. Deeper
/// than that a cell is a few pixels across, and draws a folder glyph instead.
const PLATE_DEPTH: usize = 2;

#[component]
pub(crate) fn FolderCard(state: AppState, shelf: Shelf) -> impl IntoView {
    // The prop is the shelf the `For` keyed this row on, and a keyed row is not
    // re-created when the shelf's CONTENTS change — a book filed into it, a
    // rename, a shelf nested inside. So everything that can move is read back
    // out of the state by id, and the prop supplies only the identity plus the
    // two facts a rescan owns and a reader cannot change from here. The press
    // contract, the drop registration and the state classes are the shelf's
    // one item shell (see `crate::features::library::shelf_item`); what is left
    // here is the folder's own content.
    let id = shelf.id.clone();

    let name = state.library.shelf_name_signal(&id);

    // Two counts, one line: what the folder holds, and what it holds it in.
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

    // Whether this shelf's rung is still tracked: the breathing dot that says
    // "this shelf may fill itself". The question is the rung's and not the whole
    // import's, so a subfolder turned off under a watched tree stops breathing
    // while the tree above it keeps watching. Reactive, because the answer lives
    // in two lists a hand can change while this card is on screen.
    let dot_id = id.clone();
    let watched = Signal::derive(move || state.library.shelf_tracked(&dot_id));

    // The membership the plate's check mark paints from — the same set the
    // shell's own selected class reads, and the one `SelectionCheck` marks.
    let is_selected = state.library.is_selected(&id);

    // The shelf's one press contract, the same wiring a book wears (see
    // `crate::features::library::gestures`) with the folder's own answers:
    // "open" drills the breadcrumb route, and the right-click asks about a
    // folder. A set being selected is not a reason to refuse a drag: lifting
    // one of three held folders is the whole of a multi-drag.
    // The folder half of the reveal: a folder link's tap lights the folder
    // itself, on the same signal and nonce a book's reveal rides.
    let reveal_class = state.library.is_revealed(&id);

    let open_id = id.clone();
    let target_id = id.clone();
    let policy = ShelfItemPolicy {
        id: id.clone(),
        label: Signal::derive(move || format!("the {} shelf", name.get())),
        draggable: Signal::derive(|| true),
        open: Callback::new(move |_| state.library.shelf.set(open_id.clone())),
        menu_target: Callback::new(move |_| MenuTarget::Folder {
            id: target_id.clone(),
        }),
        // A folder's lift is a nesting, which writes a parent rather than a
        // membership: there is no list to lift it off.
        container: None,
    };

    view! {
        <ShelfItemShell
            state=state
            vocab=SeamVocab::FolderCard
            base_class="folder-card"
            policy=policy
            extra_classes=vec![("folder-reveal".to_string(), reveal_class)]
        >
            <div class="folder-thumb-grid">
                <Plate state=state shelf_id=id.clone() depth=0 />
                <SelectionCheck state=state selected=is_selected />
            </div>
            <div class="folder-meta">
                <span class="folder-name" title=move || { name.get() }>
                    {move || name.get()}
                </span>
                <span class="folder-count">{move || summary(counts.get())}</span>
            </div>
            <div class="folder-badges">
                {move || {
                    watched.get().then(|| {
                        view! {
                            <span class="folder-watched" title="Watched for new books"></span>
                        }
                    })
                }}
            </div>
        </ShelfItemShell>
    }
}

/// What fills one cell of a plate: a folder, previewed as a plate of its own, or
/// a book, previewed as its cover.
#[derive(Clone)]
enum PlateItem {
    Folder(String),
    Book(Book),
}

/// The first four things inside a shelf, in the order the grid would show them —
/// folders first, then books — because a plate that disagreed with the page about
/// what is inside the folder would be a preview of something else.
///
/// Books and folders share the four cells rather than each having their own four:
/// a folder holding two folders and a book is three cells full and one empty,
/// exactly as the reader will find it.
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
    // The plate is covers, and a link has none: it is a pointer at a book
    // whose own row is in this list already when the book is inside the folder.
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

/// One folder's plate: four cells, filled in order, the rest empty.
///
/// Recursive on purpose. A cell that holds a folder holds that folder's OWN plate
/// — a folder of three books previews as three covers and an empty cell, inside
/// the cell that previews it — because "what is inside this folder" is the same
/// question at every depth, and a preview that flattened the subtree would show
/// covers the reader will not find where the preview put them.
///
/// The recursion stops at [`PLATE_DEPTH`]: a cell four plates deep is a few pixels
/// of something, and below that a folder is drawn as a folder rather than as a
/// smear. The tree is finite — `library_core::shelf::sanitize` sees to that — but
/// four cells per level is four to the power of the depth, and a preview is not
/// worth an exponent.
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
                            // The inner grid is the cell's own: a plate is four
                            // cells and nothing else, so a plate inside a cell
                            // needs the container that makes four cells a plate.
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

/// One cell holding a book: the cached cover when there is one, and the empty
/// cell's hatch when there is not yet — a cover is rendered away from the reader,
/// so "nothing cached yet" is a state the plate has to look deliberate in.
#[component]
fn CoverCell(state: AppState, book: Book) -> impl IntoView {
    let path = book.path().to_string();
    let empty_path = path.clone();
    let alt = book.title();
    // A book whose address died wears the card's own grey on the plate too:
    // the plate is the folder's answer to "what is inside it", and a dead file
    // inside is a fact the preview should not paint in full colour. Read at
    // the build rather than tracked here — the plate's items signal re-fires
    // on every change to the books list, a path check's among them, and the
    // cell this rebuilds is the cell that knows.
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
                            // Not natively draggable; see `book_card`. A plate is
                            // four of these, and any one of them taking the pointer
                            // would take it away from the folder's own gesture.
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

/// The line under the name: what the folder holds, and what it holds it in.
///
/// Both halves, because "3 books" on a folder with two shelves inside it is an
/// answer to a question the reader did not ask — those shelves are the rest of
/// the library down that path, and a count that leaves them out reads as a
/// folder that is nearly empty.
///
/// Shared with the list's tree rows (`crate::features::library::list`), because
/// a folder that counted itself differently at the two densities would be two
/// answers to "what is in here".
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
