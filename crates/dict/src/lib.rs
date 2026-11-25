//! Offline dictionaries (L1): packs, downloads, lookups.
//! See `docs/study.md` section Dictionary.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]
// The browser lists packs only; what serves downloads sits unused.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_mut))]

mod entry;
mod lang;
mod packs;
mod route;
mod search;
mod service;

#[cfg(not(target_arch = "wasm32"))]
mod build;
#[cfg(not(target_arch = "wasm32"))]
mod shelf;
#[cfg(not(target_arch = "wasm32"))]
mod store;

pub use entry::Entry;
pub use lang::name as language;
pub use packs::{PACKS, Pack};
pub use route::{HUB, Pair, languages};
pub use search::{WordMatch, fold};
pub use service::{Dictionaries, Found, Query, Reply, State, Ticket};

#[cfg(not(target_arch = "wasm32"))]
pub use build::build;
#[cfg(not(target_arch = "wasm32"))]
pub use service::Waker;
#[cfg(not(target_arch = "wasm32"))]
pub use shelf::{CARD_ROWS, SEARCH_ROWS, db_file};
