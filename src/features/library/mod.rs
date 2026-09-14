//! The library feature: the `/` route page, the shelf it shows, and the surfaces that fill
//! it.
//!
//! Grouped by what a reader does rather than by file count: [`page`] is the route and its
//! modal hosts, [`content`] the state the page is in and the one order every view renders,
//! [`grid`] and [`list`] the two densities, and the rest the cards, menus and sheets a reader
//! opens.

pub mod add_menu;
pub mod already_imported_modal;
pub mod book_card;
pub mod breadcrumb;
pub mod conflict_modal;
pub mod content;
pub mod context_menu;
pub mod copy_modal;
pub mod cover_thumb;
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
pub mod shelf_item;
pub mod titlebar_search;
pub mod view_menu;

pub use page::LibraryPage;
