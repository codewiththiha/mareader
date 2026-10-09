//! What one dictionary row is, and the order the menu shows rows in.

use serde::{Deserialize, Serialize};

use crate::pos::{CanonPos, PosMatch, parse_tags, rank_tags};

/// Where a row matched: how tightly the word itself fit the ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordMatch {
    /// The word is the ask.
    Exact,
    /// The word starts with the ask.
    Prefix,
    /// The word contains the ask.
    Substring,
    /// Near the ask: a typo, an inflection away.
    Fuzzy,
}

/// One dictionary row, whichever pack wrote it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictEntry {
    /// The headword; English in every shipped pack.
    pub word: String,
    /// The source's own POS cell, kept as written.
    pub pos_raw: Option<String>,
    /// The cell parsed to canon tags; empty when the cell is blank.
    pub tags: Vec<CanonPos>,
    /// The other-language word or gloss.
    pub definition: String,
    pub romanization: Option<String>,
    /// The sense this translation sits under, when the pack carries one.
    pub sense: Option<String>,
    /// Which pack answered.
    pub pack: String,
    /// The bridge word, when two packs carried the reader here.
    pub via: Option<String>,
    /// How the headword matched the ask.
    pub word_match: WordMatch,
}

impl DictEntry {
    /// A row's POS verdict for a detected role; `Bare` with no role.
    pub fn pos_rank(&self, detected: Option<CanonPos>) -> PosMatch {
        detected
            .map(|role| rank_tags(role, &self.tags))
            .unwrap_or(PosMatch::Bare)
    }

    /// Parse a raw POS cell into the entry's tags.
    pub fn retag(mut self, raw: Option<String>) -> Self {
        self.tags = raw.as_deref().map(parse_tags).unwrap_or_default();
        self.pos_raw = raw;
        self
    }
}

/// The sort key: POS fit first, then how the word matched.
/// Lower sorts earlier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntryRank {
    pub pos: PosMatch,
    pub word: WordMatch,
}

impl EntryRank {
    pub fn of(entry: &DictEntry, detected: Option<CanonPos>) -> Self {
        Self {
            pos: entry.pos_rank(detected),
            word: entry.word_match,
        }
    }
}

/// Order rows: role fit first, tight word fits next.
pub fn order_entries(entries: &mut [DictEntry], detected: Option<CanonPos>) {
    entries.sort_by_key(|entry| EntryRank::of(entry, detected));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(word: &str, pos: &str, word_match: WordMatch) -> DictEntry {
        DictEntry {
            word: word.to_string(),
            pos_raw: Some(pos.to_string()),
            tags: parse_tags(pos),
            definition: format!("def-{word}"),
            romanization: None,
            sense: None,
            pack: "test".into(),
            via: None,
            word_match,
        }
    }

    #[test]
    fn the_detected_role_decides_the_card_order() {
        let mut rows = vec![
            entry("run", "noun", WordMatch::Exact),
            entry("run", "md", WordMatch::Exact),
            entry("run", "v,n", WordMatch::Exact),
            entry("runner", "noun", WordMatch::Prefix),
        ];
        // Exact first, then family, then bare; fuzzy words sort last.
        order_entries(&mut rows, Some(CanonPos::Verb));
        assert_eq!(rows[0].tags, vec![CanonPos::Verb, CanonPos::Noun]);
        assert_eq!(rows[1].pos_rank(Some(CanonPos::Verb)), PosMatch::Family);
        assert_eq!(rows[2].pos_rank(Some(CanonPos::Verb)), PosMatch::Bare);
        assert_eq!(rows[3].word_match, WordMatch::Prefix);
    }

    #[test]
    fn without_a_detected_role_the_word_carries_the_order() {
        let mut rows = vec![
            entry("run", "adj", WordMatch::Fuzzy),
            entry("run", "noun", WordMatch::Exact),
        ];
        order_entries(&mut rows, None);
        assert_eq!(rows[0].word_match, WordMatch::Exact);
    }

    #[test]
    fn a_multi_tag_cell_answers_whichever_role_came_in() {
        let row = entry("light", "v,n", WordMatch::Exact);
        assert_eq!(row.pos_rank(Some(CanonPos::Verb)), PosMatch::Exact);
        assert_eq!(row.pos_rank(Some(CanonPos::Noun)), PosMatch::Exact);
        assert_eq!(row.pos_rank(Some(CanonPos::Adverb)), PosMatch::Bare);
    }
}
