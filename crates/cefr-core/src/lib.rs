//! The vocabulary highlighter's core: one tokenizer, band rule, cache and
//! the walk both painters share.

pub mod cache;
pub mod plan;
pub mod text;

pub use cache::LevelCache;
pub use plan::{PlannedWord, Walk, keys_of};
pub use text::{
    MAX_WORD_CHARS, Span, contraction_base, hyphen_parts, is_english_ascii, lookup_candidates,
    normalize, sentence_around, tokenize,
};
