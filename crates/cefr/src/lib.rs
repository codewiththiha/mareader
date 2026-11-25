//! Word levels (L1): the CEFR dataset, hard words, a word's role.
//! See `docs/study.md` section Vocabulary.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]
// The browser plans with no answers; what serves them sits unused.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_mut))]

mod levels;
mod roles;
mod service;
mod source;
mod text;

#[cfg(not(target_arch = "wasm32"))]
mod store;

pub use levels::{CACHE_CAP, Levels, Plan, band, plan, word_band};
pub use roles::Role;
pub use service::{Part, Polled, State, Ticket, Vocabulary};
pub use source::{DATASET, Source, TAGGER};
pub use text::{MAX_WORD_CHARS, Span, candidates, normalize, tokenize};

#[cfg(not(target_arch = "wasm32"))]
pub use service::Waker;
#[cfg(not(target_arch = "wasm32"))]
pub use store::{Sense, Store, build};

/// A CEFR label for a band, 1 to 6; "?" otherwise.
pub fn label(band: u8) -> &'static str {
    cefr::level::level_to_cefr(f64::from(band))
}
