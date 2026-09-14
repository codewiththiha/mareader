//! The library's domain: what a book is, how the app holds one, and the rules
//! that decide what a folder scan does next.
//!
//! Everything here is pure — no filesystem, no wasm, no DOM, no leptos — so a
//! host test can hold the decisions to account (`cargo test -p library-core`).
//! Walking a folder, measuring a file and copying one into the store is the
//! Tauri shell's business (its `commands` module); rendering the shelf and the
//! import sheets is `src/features/library`.
//!
//! A book is an ADDRESS, not a copy: a [`book::Origin::Linked`] book is the
//! path it was opened from, and nothing in this crate ever moves a file the
//! user owns. [`book::Origin::Stored`] is the opt-in other way round.
//!
//! A shelf holds [`book::Row`]s. Questions about a row's CONTENT — a
//! fingerprint, an address, a resume point — are asked of the book rows
//! ([`book::book_rows`]), because a [`book::Row::Link`] has none of the three;
//! questions about a row's PLACE — membership, a drag, a removal — are asked of
//! the row, whichever kind it is.

pub mod blob;
pub mod book;
pub mod conflict;
pub mod folder;
pub mod governance;
pub mod hash;
pub mod id;
pub mod ledger;
pub mod query;
pub mod scan;
pub mod shape;
pub mod shelf;
pub mod sort;
pub mod store;
pub mod text;
pub mod tracking;
pub mod view;
pub mod wire;

/// Test fixtures for the crate's own tests and for a dependent crate's
/// `test-util` dev-dependency.
#[cfg(any(test, feature = "test-util"))]
pub mod testkit;

