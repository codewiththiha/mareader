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
mod batch;
mod deck;
mod explain;
mod gloss;
mod judge;
mod mode;
pub mod page;
mod space;

pub use answer::WordInfo;
pub use batch::{
    Batch, BatchCfg, BatchUnit, LIGHT_CHARS, MAX_PAGES, build_batches, deck_seconds, mmss,
    range_label, seconds_after_known,
};
pub use deck::{CardCandidate, Deck, SeenWord, WordClass, WordKey, build_deck, repeats_word};
pub use explain::{Canned, Explainer, Failure, Unavailable, Why};
pub use gloss::{CONTEXT_CHARS, MAX_GLOSS_CHARS, context, word_in};
pub use judge::{Judged, MAX_ATTEMPTS, Verdict, attempts_left, hint_due};
pub use mode::{Avail, CardSource, StudyEnv, StudyMode, availability, resolve_via};
pub use space::{
    LearnedBy, Scope, SpaceFilter, SpaceGroup, SpaceItem, SpaceRow, SpaceSort, WordStatus,
    band_label, export_csv, filtered, sorted, summary, to_items,
};
