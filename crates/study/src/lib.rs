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
mod batches;
mod deck;
mod explain;
mod gloss;
mod judge;
mod mode;
pub mod page;

pub use answer::WordInfo;
pub use batches::{Batch, BatchCfg, LIGHT_CHARS, MAX_PAGES, batch_of, build_batches, range_label};
pub use deck::{Candidate, Card, Deck, build_deck, deck_seconds};
pub use explain::{Canned, Explainer, Failure, Unavailable, Why};
pub use gloss::{CONTEXT_CHARS, MAX_GLOSS_CHARS, context, word_in};
pub use judge::{Judged, RecallTarget, Verdict, judge};
pub use mode::{
    Avail, BatchUnit, Cfg, Env, Fix, Mode, Source, Speed, availability, resolve_via, speed_ms,
    word_key,
};
