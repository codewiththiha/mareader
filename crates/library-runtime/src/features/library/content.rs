//! The library's content area: opening, failed, and the shelf.

use std::collections::HashSet;
use std::time::Duration;

use leptos::prelude::*;

use app_chrome::hooks::dom::by_id;
use library_core::book::Row;
use library_core::query;
use library_core::shelf::{ALL_SHELF, Shelf, children_of, find, members_of};
use library_core::sort::{self, SortKey};

use crate::features::library::context_menu::{LibraryContextMenu, LibraryMenuHost, MenuTarget};
use crate::features::library::dnd::controller::DragController;
use crate::features::library::dnd::target::{DropTargetEntry, DropTargetId, DropTargetKind};
use crate::features::library::empty_state::EmptyState;
use crate::features::library::grid::GridView;
use crate::features::library::list::ListView;
use crate::features::library::selection::{LibrarySelectBar, use_select_mode};
use crate::services::backfill_missing;
use crate::state::library::Reveal;
use app_ui::components::primitives::motion::reduced_motion::prefers_reduced_motion;

const LEVEL_DOM_ID: &str = "library-level";

/// A card cannot count its index in the DOM, so views read the order here.
#[derive(Clone, Copy)]
pub struct ShelfOrder(pub Signal<Vec<Row>>);

/// The grid renders folders before books; a card deriving that would be
/// a second answer.
#[derive(Clone, Copy)]
pub struct FolderOrder(pub Signal<Vec<Shelf>>);

const REVEAL_MS: u64 = 1600;

/// Two frames before the lookup: a reveal's grid mounts with the shelf switch.
fn install_reveal(state: crate::context::LibraryContext) {
    Effect::new(move |_| {
        let Some(Reveal { id: book_id, nonce }) = state.library.reveal.get() else {
            return;
        };
        // The seam table owns the id scheme.
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
        // The nonce guard lets a second reveal of one book re-light it.
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

/// Either the reader's switch or the platform's answer is enough.
fn scroll_may_animate(state: crate::context::LibraryContext) -> bool {
    state.settings.with_untracked(|s| s.animations.enabled) && !prefers_reduced_motion()
}

/// The order both layouts render and a drop counts against.
pub(crate) fn level_rows(state: crate::context::LibraryContext) -> Vec<Row> {
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
        let members = state.library.shelves.with(|shelves| {
            find(shelves, &shelf_id)
                .map(|s| s.books.clone())
                .unwrap_or_default()
        });
        sort::ordered(&rows, &members, SortKey::Manual, true)
    };
    sort::sort_rows(&mut list, view.sort, view.sort_asc);
    query::filter(&list, &state.library.query.get())
}

/// One answer for both densities: hiding folders in the grid would be
/// two searches.
pub(crate) fn level_folders(
    state: crate::context::LibraryContext,
    root: Option<String>,
) -> Vec<Shelf> {
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
pub(crate) fn LibraryContent(state: crate::context::LibraryContext) -> impl IntoView {
    // One derived signal, so the grid, list and empty-state line agree.
    let order = Signal::derive(move || level_rows(state));
    provide_context(ShelfOrder(order));
    let folders = Signal::derive(move || level_folders(state, None));
    provide_context(FolderOrder(folders));
    // The registry hit-tests in reverse, so later cards win.
    let drag = use_context::<DragController>().expect("the library page installs the drag session");
    let menu = use_context::<LibraryMenuHost>().expect("the library page provides the menu");
    drag.registry.register(DropTargetEntry {
        id: DropTargetId(DropTargetKind::Level, String::new()),
        dom_id: LEVEL_DOM_ID.to_string(),
        shelf: None,
    });
    // The queue skips what it has: a question, asked once per mount.
    Effect::new(move |_| {
        backfill_missing(state);
    });
    install_reveal(state);
    // One listener here, not per card: N would race to leave the mode.
    use_select_mode(state);

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
            <Show when=move || has_anything.get() fallback=move || view! { <EmptyState state=state /> }>
                    <div
                        id=LEVEL_DOM_ID
                        class="min-h-0 flex-1 overflow-y-auto pt-12"
                        // A card's right-click stops propagating, so this
                        // hears the space between cards.
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
            <LibrarySelectBar state=state />
            <LibraryContextMenu state=state />
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::LibraryContext;
    use library_core::book::Book;
    use library_core::sort::SortKey;

    fn library(rows: Vec<Row>, shelves: Vec<Shelf>) -> (Owner, LibraryContext) {
        let owner = Owner::new();
        owner.set();
        let state = LibraryContext::default();
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
            vec![
                book("a", "Dune"),
                book("b", "Neuromancer"),
                book("c", "Hyperion"),
            ],
            vec![shelf("s", "Fiction", &["b"], None)],
        );
        assert_eq!(ids(&level_rows(state)), vec!["a", "c"]);
    }

    #[test]
    fn a_shelf_shows_its_own_members_in_its_own_order() {
        let (_owner, state) = library(
            vec![
                book("a", "Dune"),
                book("b", "Neuromancer"),
                book("c", "Hyperion"),
            ],
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
            vec![
                book("c", "Hyperion"),
                book("a", "Dune"),
                book("b", "Endymion"),
            ],
            vec![],
        );
        state.library.view.update(|v| {
            v.sort = SortKey::Title;
            v.sort_asc = true;
        });
        assert_eq!(ids(&level_rows(state)), vec!["a", "b", "c"]);
        state.library.query.set("dy".to_string());
        assert_eq!(
            ids(&level_rows(state)),
            vec!["b"],
            "fuzzy, and still in order"
        );
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
            level_folders(state, None)
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
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
            level_folders(state, None)
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
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
