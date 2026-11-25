//! The AI dictionary's entry: a meaning an AI wrote on this machine.
//!
//! A meaning is kept by its word, class and language together, so the
//! verb "book" and the noun "book" are two entries, and a Japanese
//! meaning never stands in for an English one. `ai_store` keeps them.

use crate::search::fold;

/// The pack id AI rows carry.
pub const AI_PACK: &str = "ai";

/// One meaning an AI wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    pub word: String,
    /// Its class by name, as `noun`.
    pub pos: String,
    /// The meaning's language code, as `en` or `ja`.
    pub lang: String,
    pub meaning: String,
    pub synonyms: Vec<String>,
    pub usages: Vec<String>,
    /// The provider and model that wrote it.
    pub model: String,
    /// When it was saved, in wall milliseconds.
    pub added: u64,
}

impl Saved {
    /// Its key: the folded word, the class and the language.
    pub fn key(&self) -> (String, String, String) {
        (fold(&self.word), class(&self.pos), self.lang.clone())
    }

    /// Whether `other` says the same, whoever wrote it and when.
    pub fn same_as(&self, other: &Saved) -> bool {
        self.key() == other.key()
            && self.meaning == other.meaning
            && self.synonyms == other.synonyms
            && self.usages == other.usages
    }
}

/// A class as kept: trimmed and lower case.
pub(crate) fn class(pos: &str) -> String {
    pos.trim().to_lowercase()
}
