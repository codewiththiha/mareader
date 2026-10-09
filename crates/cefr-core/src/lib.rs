//! The vocabulary highlighter's core: tokenizer, planner, level cache.

pub mod cache;
pub mod plan;
pub mod text;

pub use cache::LevelCache;
pub use cefr::level::level_band;
pub use plan::{Plan, Planned, plan, plan_tokens, planned};
pub use text::{
    MAX_WORD_CHARS, Span, contraction_base, hyphen_parts, is_english_ascii, lookup_candidates,
    normalize, sentence_around, tokenize,
};

/// The band a dataset answer carries; a word with no answer is band zero.
pub fn band(level: Option<f64>) -> u8 {
    level.map(level_band).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unanswered_word_is_band_zero_never_a1() {
        assert_eq!(band(None), 0);
        assert_eq!(band(Some(1.0)), 1);
        assert_eq!(band(Some(5.4)), 5);
        // The rounding is the dataset crate's own rule.
        assert_eq!(band(Some(f64::NAN)), 0);
    }
}
