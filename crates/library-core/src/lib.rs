//! The library's domain: books, how the app holds them, and the scan
//! rules.

pub mod blob;
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
