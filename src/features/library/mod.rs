//! The library feature: the `/` route page, the shelf it shows, and the surfaces
//! that fill it.
//!
//! Grouped by what a reader does rather than by file count:
//!
//!   * [`page`] — the route: the title bar's three slots, the modal hosts, and the
//!     drag session with the layer that draws what it is holding
//!   * [`content`] — the state the page is in (opening, failed, shelf) and the one
//!     order every view below it renders, plus the one level query both densities
//!     ask for their doors
//!   * [`facts`] — the facts about a book a card and a row both paint, read back
//!     out of the library by id rather than taken from a keyed prop that a content
//!     change does not re-create
//!   * [`grid`] / [`list`] / [`folder_card`] / [`book_card`] — the shelf itself
//!   * [`gestures`] — the press contract every one of those items shares: one
//!     wrapper's decision, the drag session's endpoints, the keyboard's halves
//!     and the right-click's ask, wired once
//!   * [`shelf_item`] — the element that contract paints on: one outer div
//!     wearing the handlers, the registration and the state classes, so the
//!     six surfaces differ only in vocabulary and content
//!   * [`entry`] — the door every one of those surfaces goes through: a
//!     descriptor of what the item IS, and the shell that paints the reveal,
//!     the state classes and the registration from it, so the six call sites
//!     stop spelling the same wiring out by hand
//!   * [`add_menu`] / [`empty_state`] — the two ways in, on the one trigger
//!     ([`add_menu::AddMenuButton`]) wearing three faces
//!   * [`cover_thumb`] — the one painter of a cached cover, and a surface's
//!     fallback beside it
//!   * [`import_modal`] — what a folder import is allowed to be
//!   * [`relink_modal`] — a book that is not where the library left it: the
//!     two doors that point it at the file it is now, and the Cancel that
//!     leaves the missing book exactly as missing as it was
//!   * [`remove_modal`] — what a removal costs, itemised
//!   * [`rename_modal`] — the name the shelf shows, retyped: one field for a
//!     book, a link or a shelf, and a display name is all it writes — the id,
//!     the address and the file on disk keep what they have
//!   * [`conflict_modal`] — the shelf already holds that book: the
//!     duplicate/replace/merge question, and the second ask a replace owes;
//!     a folder merge's per-file asks wear its compact sheet, and a loose
//!     file of a read-at-place folder wears the covered sheet's two answers.
//!     One file per question, with the strings all three print in [`info`]
//!     beside them
//!
//! [`info`]: crate::features::library::conflict_modal
//!   * [`shelf_conflict_modal`] — the level already holds that NAME: the
//!     folder question an import asks before its walk, and the mode switch a
//!     read-at-place folder asks when it is re-picked as copies
//!   * [`departure_modal`] — a hand taking a read-at-place shelf off the seat
//!     its folder's tree names: the move becomes the library's own copy, asked
//!     before the copies are made
//!   * [`already_imported_modal`] — a folder the library already reads in
//!     place, named and lit up; a report for two of its three sentences, and
//!     for the third a question, because a member of the tree is standing
//!     outside it and can be put back
//!   * [`selection`] — the hold that starts a multi-select, the mark that says an
//!     item is in it, and the bar that acts on the set
//!   * [`context_menu`] — the right-click: one menu per kind of thing under the
//!     pointer, and the one host every surface asks
//!   * [`progress_dock`] — the run's ring, in the corner
//!   * [`view_menu`] / [`breadcrumb`] / [`titlebar_search`] — the bar's three jobs,
//!     the last with [`search_suggest`] under it: the ranked, fuzzy answer to a
//!     half-typed query
//!   * [`dnd`] — the drag that moves things: the session, the targets it can land
//!     on, the table that decides what a drop means, and the overlay it draws
//!
//! A level of the library is the same shape at every depth: [`grid`] renders the
//! folders filed at this level and then the books on it, and a folder card is one
//! cell of the grid exactly like a book's — which is what lets a shelf be filed
//! inside another one. [`content`] derives the level once and provides both halves.
//!
//! None of these decides anything: the rules live in `library_core` and the
//! operations in `crate::services::library`, so a view here can be replaced
//! without the library's behaviour moving with it. [`dnd`] is the exception that
//! proves the shape — it decides what a DROP means, which is a question about a
//! gesture rather than about the library, and it hands the answer to the same
//! services every menu row uses.

pub mod add_menu;
pub mod already_imported_modal;
pub mod book_card;
pub mod breadcrumb;
pub mod conflict_modal;
pub mod content;
pub mod context_menu;
pub mod cover_thumb;
pub mod departure_modal;
pub mod dnd;
pub mod empty_state;
pub mod entry;
pub mod facts;
pub mod folder_card;
pub mod gestures;
pub mod grid;
pub mod import_modal;
pub mod link_card;
pub mod list;
pub mod page;
pub mod progress_dock;
pub mod relink_modal;
pub mod remove_modal;
pub mod rename_modal;
pub mod search_suggest;
pub mod selection;
pub mod shelf_conflict_modal;
pub mod shelf_item;
pub mod titlebar_search;
pub mod view_menu;

pub use page::LibraryPage;
