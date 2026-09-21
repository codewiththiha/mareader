//! The library's domain: what a book is, how the app holds one, and the rules
//! that decide what a folder scan does next.
//!
//! Pure domain logic — no filesystem, no wasm, no DOM, no leptos — so every
//! decision here is testable on the host. Folder walking and file IO live in
//! the Tauri shell's `commands` module; rendering lives in
//! `src/features/library`.
//!
//! A book is an address, not a copy: [`book::Origin::Linked`] points at the
//! path the user opened and this crate never moves a file the user owns;
//! [`book::Origin::Stored`] is the opt-in copy-into-store mode.
//!
//! A shelf holds [`book::Row`]s. Content questions (fingerprint, address,
//! resume point) go through [`book::book_rows`] because a [`book::Row::Link`]
//! carries none of them; place questions (membership, drag, removal) go
//! through the row itself, whichever kind it is.

pub mod blob;
pub mod covers;
pub mod book;
pub mod conflict;
pub mod folder;
pub mod governance;
pub mod hash;
pub mod id;
pub mod ledger;
pub mod paths;
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

/// Test fixtures, shared with dependent crates through the `test-util` feature.
#[cfg(any(test, feature = "test-util"))]
pub mod testkit;
