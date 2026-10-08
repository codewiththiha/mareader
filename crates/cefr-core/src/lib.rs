//! The vocabulary highlighter's core: one tokenizer, band rule, cache.

pub mod cache;
pub mod text;

pub use cache::LevelCache;
pub use text::{
    MAX_WORD_CHARS, Span, contraction_base, hyphen_parts, is_english_ascii, lookup_candidates,
    normalize, sentence_around, tokenize,
};
