//! Reading aids as pure rules (L1); see `docs/study.md`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod answer;
mod explain;
mod gloss;
pub mod mode;
pub mod page;

pub use answer::WordInfo;
pub use explain::{Canned, Explainer, Failure, Unavailable, Why};
pub use gloss::{CONTEXT_CHARS, MAX_GLOSS_CHARS, context, word_in};
