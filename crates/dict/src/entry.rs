//! One dictionary row as the reader sees it, and the order rows show in.

use crate::search::{WordMatch, fold};

/// One answer, whichever pack gave it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The headword, on the pack's source side.
    pub word: String,
    /// The pack's part-of-speech cell, as written.
    pub pos: Option<String>,
    /// The other side: a word or a short gloss.
    pub definition: String,
    pub romanization: Option<String>,
    /// Which sense of the headword this translates.
    pub sense: Option<String>,
    pub pack: &'static str,
    /// The English word between two packs, on a bridged answer.
    pub via: Option<String>,
    /// How the side that was asked fits the ask.
    pub fit: WordMatch,
    /// Asked from the definition side: the answer is the headword.
    pub flipped: bool,
}

impl Entry {
    /// The side that answers the ask.
    pub fn answer(&self) -> &str {
        if self.flipped {
            &self.word
        } else {
            &self.definition
        }
    }

    /// The side that was asked.
    pub fn asked(&self) -> &str {
        if self.flipped {
            &self.definition
        } else {
            &self.word
        }
    }
}

/// Tight fits first, pack order within a fit; repeated rows show once.
pub fn order(entries: &mut Vec<Entry>, limit: usize) {
    entries.sort_by_key(|e| e.fit);
    let mut seen = std::collections::HashSet::new();
    entries.retain(|e| {
        let key = (
            fold(&e.word),
            e.pos.clone(),
            fold(&e.definition),
            e.sense.clone(),
        );
        seen.insert(key)
    });
    entries.truncate(limit);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(word: &str, pos: &str, definition: &str, fit: WordMatch) -> Entry {
        Entry {
            word: word.into(),
            pos: Some(pos.into()),
            definition: definition.into(),
            romanization: None,
            sense: None,
            pack: "test",
            via: None,
            fit,
            flipped: false,
        }
    }

    #[test]
    fn exact_rows_lead_and_repeats_show_once() {
        let mut rows = vec![
            entry("runner", "n", "a", WordMatch::Prefix),
            entry("run", "v", "b", WordMatch::Exact),
            entry("Run", "v", "b", WordMatch::Exact),
            entry("run", "n", "c", WordMatch::Exact),
        ];
        order(&mut rows, 10);
        let words: Vec<_> = rows.iter().map(|e| e.definition.as_str()).collect();
        assert_eq!(words, ["b", "c", "a"]);
        order(&mut rows, 1);
        assert_eq!(rows.len(), 1);
    }
}
