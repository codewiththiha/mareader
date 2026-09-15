//! The library's content area: what the page shows under its title bar.
//!
//! Three states — a document opening, a document that failed to open, and the
//! shelf itself — live here rather than on the reader's page because this
//! route is where the app lands on launch, after a close, and after a failed
//! open.

use std::collections::HashSet;
use std::time::Duration;

use leptos::prelude::*;

use app_chrome::hooks::dom::by_id;
use pdf_engine::types::DocStatus;
use library_core::query;
use library_core::shelf::{ALL_SHELF, Shelf, children_of, find, members_of};
use library_core::sort::{self, SortKey};
use library_core::book::Row;

use crate::components::primitives::feedback::CenteredLoader;
use crate::components::primitives::motion::reduced_motion::prefers_reduced_motion;
use crate::features::library::context_menu::{LibraryContextMenu, LibraryMenuHost, MenuTarget};
use crate::features::library::dnd::controller::DragController;
use crate::features::library::dnd::target::{DropTargetEntry, DropTargetId, DropTargetKind};
use crate::features::library::empty_state::EmptyState;
use crate::features::library::grid::GridView;
use crate::features::library::list::ListView;
use crate::features::library::selection::{LibrarySelectBar, use_select_mode};
use crate::services::library::backfill_missing;
use crate::state::library::Reveal;
use crate::state::AppState;

const LEVEL_DOM_ID: &str = "library-level";

/// Provided here, read by the views, the selection bar and
/// `crate::features::library::dnd::commit`: a card cannot work its index out
/// of the DOM without counting siblings, which would be a second definition
/// of the order.
#[derive(Clone, Copy)]
pub struct ShelfOrder(pub Signal<Vec<Row>>);

/// Provided beside [`ShelfOrder`] for the same reason: the grid renders
/// folders before books, and a card that derived the level itself would be a
/// second answer to "what is here".
#[derive(Clone, Copy)]
pub struct FolderOrder(pub Signal<Vec<Shelf>>);

const REVEAL_MS: u64 = 1600;

/// Two animation frames before the lookup, not one: the reveal usually
/// arrives with a shelf switch, and the grid it scrolls is the one the switch
/// mounts — not yet in the DOM in the frame the signal was written.
fn install_reveal(state: AppState) {
    Effect::new(move |_| {
        let Some(Reveal { id: book_id, nonce }) = state.library.reveal.get() else {
            return;
        };
        // The seam table owns the id scheme, so the reveal reads it rather
        // than re-spelling the prefixes.
        let dom_id = crate::features::library::shelf_item::reveal_dom_id(
            library_core::id::is_shelf(&book_id),
            state.library.view.with_untracked(|v| v.is_list()),
            &book_id,
        );
        let smooth = scroll_may_animate(state);
        request_animation_frame(move || {
            request_animation_frame(move || {
                let Some(node) = by_id(&dom_id) else {
                    return;
                };
                let options = web_sys::ScrollIntoViewOptions::new();
                options.set_block(web_sys::ScrollLogicalPosition::Center);
                options.set_behavior(if smooth {
                    web_sys::ScrollBehavior::Smooth
                } else {
                    web_sys::ScrollBehavior::Auto
                });
                node.scroll_into_view_with_scroll_into_view_options(&options);
            });
        });
        // The nonce guard lets a second reveal of the same book re-light it:
        // without it the first reveal's clear would put out the second.
        let handle = set_timeout_with_handle(
            move || {
                state.library.reveal.update(|at| {
                    if at.as_ref().is_some_and(|each| each.nonce == nonce) {
                        *at = None;
                    }
                });
            },
            Duration::from_millis(REVEAL_MS),
        )
        .ok();
        on_cleanup(move || {
            if let Some(handle) = handle {
                handle.clear();
            }
        });
    });
}

/// The reader's master switch and the platform's answer are two questions
/// with one shape: either saying no is enough.
fn scroll_may_animate(state: AppState) -> bool {
    state.settings.with_untracked(|s| s.animations.enabled) && !prefers_reduced_motion()
}

/// The order both layouts render and a drop counts against: a card lands
/// "here" at an index in what the reader is looking at. Four steps, and the
/// sequence is the point.
pub(crate) fn level_rows(state: AppState) -> Vec<Row> {
    let view = state.library.view.get();
    let shelf_id = state.library.shelf.get();
    let rows = state.library.books.get();
    let mut list = if shelf_id == ALL_SHELF {
        let terms = state.library.query.get();
        if query::is_active(&terms) {
            rows
        } else {
            let unfiled: HashSet<String> = state.library.shelves.with(|shelves| {
                members_of(&rows, shelves, ALL_SHELF)
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            });
            rows.into_iter()
                .filter(|r| unfiled.contains(r.id()))
                .collect()
        }
    } else {
        let members = state
            .library
            .shelves
            .with(|shelves| find(shelves, &shelf_id).map(|s| s.books.clone()).unwrap_or_default());
        sort::ordered(&rows, &members, SortKey::Manual, true)
    };
    sort::sort_rows(&mut list, view.sort, view.sort_asc);
    query::filter(&list, &state.library.query.get())
}

/// One answer for both densities: a search that hid matching folders in the
/// grid but kept them in the list would be two searches wearing one text box.
pub(crate) fn level_folders(state: AppState, root: Option<String>) -> Vec<Shelf> {
    let at = state.library.shelf.get();
    let terms = state.library.query.get();
    let parent = match root {
        Some(pinned) => Some(pinned),
        None => (at != ALL_SHELF).then_some(at),
    };
    state.library.shelves.with(|shelves| {
        children_of(shelves, parent.as_deref())
            .into_iter()
            .filter(|s| s.id != ALL_SHELF)
            .filter(|s| query::matches_terms(&s.name, &terms))
            .cloned()
            .collect()
    })
}

#[component]
pub(crate) fn LibraryContent(state: AppState) -> impl IntoView {
    // One derived signal, so the grid, the list and the empty-state line
    // agree about what is on screen.
    let order = Signal::derive(move || level_rows(state));
    provide_context(ShelfOrder(order));
    let folders = Signal::derive(move || level_folders(state, None));
    provide_context(FolderOrder(folders));
    // The registry hit-tests in reverse, so cards mounted after this are
    // found ahead of the space they stand on. One registration for both
    // layouts: there is one scroll container.
    let drag = use_context::<DragController>().expect("the library page installs the drag session");
    let menu = use_context::<LibraryMenuHost>().expect("the library page provides the menu");
    drag.registry.register(DropTargetEntry {
        id: DropTargetId(DropTargetKind::Level, String::new()),
        dom_id: LEVEL_DOM_ID.to_string(),
        shelf: None,
    });
    // The queue skips what it has, so this is a question rather than a
    // command. No tracked reads inside: it asks once per mount.
    Effect::new(move |_| {
        backfill_missing(state);
    });
    install_reveal(state);
    // Installed here rather than per card: one listener per card would be N
    // listeners racing to leave the same mode.
    use_select_mode(state);

    let status = state.reader.document.status;
    let error = state.reader.document.error;
    let is_list = Signal::derive(move || state.library.view.with(|v| v.is_list()));
    let has_books = Signal::derive(move || state.library.books.with(|b| !b.is_empty()));
    let has_anything = Signal::derive(move || has_books.get() || !folders.get().is_empty());
    let quiet = Signal::derive(move || {
        if !order.get().is_empty() || !folders.get().is_empty() || !has_books.get() {
            return None;
        }
        let searching = state.library.query.with(|q| !q.trim().is_empty());
        Some(if searching {
            "No books match this search.".to_string()
        } else {
            "This shelf is empty.".to_string()
        })
    });

    view! {
        <div class="flex h-full w-full flex-col">
            // An open cannot be aborted from here and does not need to be:
            // picking another file claims a new session stamp, and the
            // in-flight attempt drops its own tail (see
            // `crate::services::document::session`).
            <Show when=move || status.get() == DocStatus::Opening fallback=|| ()>
                <CenteredLoader />
            </Show>
            <Show when=move || status.get() == DocStatus::Error fallback=|| ()>
                <div class="flex h-full w-full items-center justify-center pt-12 text-center text-muted">
                    <p class="text-lg">
                        // Read untracked on purpose: the sentence is a
                        // snapshot of the failed attempt, and a tracked read
                        // inside `unwrap_or_else` would only subscribe on the
                        // runs where it executes.
                        {move || {
                            error.get().unwrap_or_else(|| {
                                let kind = state
                                    .reader
                                    .document
                                    .path
                                    .get_untracked()
                                    .map_or("document", |p| {
                                        reader_core::format::format_of(&p).label()
                                    });
                                format!("Could not open this {kind}")
                            })
                        }}
                    </p>
                </div>
            </Show>
            <Show when=move || status.get() == DocStatus::Idle fallback=|| ()>
                <Show
                    when=move || has_anything.get()
                    fallback=move || view! { <EmptyState state=state /> }
                >
                    <div
                        id=LEVEL_DOM_ID
                        class="min-h-0 flex-1 overflow-y-auto pt-12"
                        // "New shelf" and "select all" belong to the level;
                        // a card's own right-click stops propagating, so this
                        // only hears the space between cards.
                        on:contextmenu=move |ev: leptos::ev::MouseEvent| {
                            ev.prevent_default();
                            menu.ask(
                                ev.client_x() as f64,
                                ev.client_y() as f64,
                                MenuTarget::Level,
                            );
                        }
                    >
                        <div class="mx-auto w-full max-w-6xl px-6 py-8">
                            {move || {
                                if is_list.get() {
                                    view! { <ListView state=state /> }.into_any()
                                } else {
                                    view! { <GridView state=state /> }.into_any()
                                }
                            }}
                            {move || {
                                quiet.get().map(|line| {
                                    view! {
                                        <p class="py-16 text-center text-sm text-muted">{line}</p>
                                    }
                                })
                            }}
                        </div>
                    </div>
                </Show>
            </Show>
            <LibrarySelectBar state=state />
            <LibraryContextMenu state=state />
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::book::Book;
    use library_core::sort::SortKey;

    fn library(rows: Vec<Row>, shelves: Vec<Shelf>) -> (Owner, AppState) {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        state.library.books.set(rows);
        state.library.shelves.set(shelves);
        (owner, state)
    }

    fn book(id: &str, title: &str) -> Row {
        Row::Book(Book {
            title: Some(title.to_string()),
            ..library_core::testkit::markdown_book(id)
        })
    }

    fn shelf(id: &str, name: &str, members: &[&str], parent: Option<&str>) -> Shelf {
        library_core::testkit::shelf(id, name, members, parent)
    }

    fn ids(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.id()).collect()
    }

    #[test]
    fn the_root_shows_the_rows_no_shelf_holds() {
        let (_owner, state) = library(
            vec![book("a", "Dune"), book("b", "Neuromancer"), book("c", "Hyperion")],
            vec![shelf("s", "Fiction", &["b"], None)],
        );
        assert_eq!(ids(&level_rows(state)), vec!["a", "c"]);
    }

    #[test]
    fn a_shelf_shows_its_own_members_in_its_own_order() {
        let (_owner, state) = library(
            vec![book("a", "Dune"), book("b", "Neuromancer"), book("c", "Hyperion")],
            vec![shelf("s", "Fiction", &["c", "a"], None)],
        );
        state.library.shelf.set("s".to_string());
        assert_eq!(
            ids(&level_rows(state)),
            vec!["c", "a"],
            "the member list IS the order, not the library's"
        );
    }

    #[test]
    fn a_member_naming_a_row_that_went_is_skipped_not_holed() {
        let (_owner, state) = library(
            vec![book("a", "Dune")],
            vec![shelf("s", "Fiction", &["gone", "a"], None)],
        );
        state.library.shelf.set("s".to_string());
        assert_eq!(ids(&level_rows(state)), vec!["a"]);
    }

    #[test]
    fn a_link_is_a_row_the_level_renders() {
        let rows = vec![
            book("a", "Dune"),
            Row::link("l1".into(), "Dune".into(), "a".into(), 1),
        ];
        let (_owner, state) = library(rows, vec![shelf("s", "Fiction", &["l1", "a"], None)]);
        state.library.shelf.set("s".to_string());
        assert_eq!(ids(&level_rows(state)), vec!["l1", "a"]);
    }

    #[test]
    fn the_sort_runs_before_the_filter_so_a_query_never_reorders() {
        let (_owner, state) = library(
            vec![book("c", "Hyperion"), book("a", "Dune"), book("b", "Endymion")],
            vec![],
        );
        state.library.view.update(|v| {
            v.sort = SortKey::Title;
            v.sort_asc = true;
        });
        assert_eq!(ids(&level_rows(state)), vec!["a", "b", "c"]);
        state.library.query.set("dy".to_string());
        assert_eq!(ids(&level_rows(state)), vec!["b"], "fuzzy, and still in order");
        state.library.query.set(String::new());
        assert_eq!(ids(&level_rows(state)), vec!["a", "b", "c"]);
    }

    #[test]
    fn a_search_from_the_root_reaches_into_every_folder() {
        let (_owner, state) = library(
            vec![book("a", "Dune"), book("b", "Neuromancer")],
            vec![shelf("s", "Fiction", &["b"], None)],
        );
        state.library.query.set("neuro".to_string());
        assert_eq!(ids(&level_rows(state)), vec!["b"]);
    }

    #[test]
    fn an_empty_level_is_empty_rather_than_everything() {
        let (_owner, state) = library(
            vec![book("a", "Dune")],
            vec![shelf("s", "Fiction", &[], None)],
        );
        state.library.shelf.set("s".to_string());
        assert!(level_rows(state).is_empty());
    }

    #[test]
    fn a_shelf_the_list_no_longer_holds_shows_nothing() {
        let (_owner, state) = library(vec![book("a", "Dune")], vec![]);
        state.library.shelf.set("gone".to_string());
        assert!(level_rows(state).is_empty());
    }

    #[test]
    fn the_doors_on_a_level_are_the_shelves_filed_directly_inside_it() {
        let (_owner, state) = library(
            vec![],
            vec![
                shelf("top", "Fiction", &[], None),
                shelf("inner", "Sci-fi", &[], Some("top")),
                shelf("other", "History", &[], None),
            ],
        );
        assert_eq!(
            level_folders(state, None).iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["top", "other"],
            "a nested shelf is its parent's door, not the root's"
        );
        let inside = level_folders(state, Some("top".to_string()));
        assert_eq!(
            inside.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["inner"]
        );
    }

    #[test]
    fn an_open_query_narrows_the_doors_by_name_with_the_books_own_rule() {
        let (_owner, state) = library(
            vec![],
            vec![
                shelf("a", "Science Fiction", &[], None),
                shelf("b", "History", &[], None),
            ],
        );
        state.library.query.set("sci".to_string());
        assert_eq!(
            level_folders(state, None).iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["a"],
            "one text box is not two searches wearing one field"
        );
    }

    #[test]
    fn a_pinned_root_walks_its_own_subtree_whatever_level_the_page_is_on() {
        let (_owner, state) = library(
            vec![],
            vec![
                shelf("top", "Fiction", &[], None),
                shelf("inner", "Sci-fi", &[], Some("top")),
            ],
        );
        state.library.shelf.set("elsewhere".to_string());
        assert_eq!(
            level_folders(state, Some("top".to_string()))
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            vec!["inner"]
        );
    }
}
