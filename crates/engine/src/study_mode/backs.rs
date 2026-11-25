//! A card's back: a dictionary's definitions or an AI's words, and the
//! backs an AI already wrote.
//!
//! [`Backs`] keeps written backs for the session, by card, sentence and
//! writer (its model and language), so a deck asked again, or a batch
//! planned again, costs no second request.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use mareader_dict::Entry;
use mareader_library::MeaningBy;
use mareader_study::mode::{Card, CardWriter, Key, Pos, Wanted, Written};

/// Definitions one card shows at most.
const DEFINITIONS: usize = 3;
/// Backs kept at most; past it the cache starts over.
const KEPT: usize = 4096;

/// A card and its back; the meaning is empty when no one had it.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub card: Card,
    pub meaning: String,
    /// The AI's synonyms and example; the dictionary gives none.
    pub synonyms: Vec<String>,
    pub usage: String,
    /// Who wrote the meaning; none when it is empty.
    pub by: Option<MeaningBy>,
}

impl Shown {
    /// A card with a dictionary's meaning.
    pub fn new(card: Card, meaning: String) -> Self {
        Self {
            by: said(&meaning, MeaningBy::Dictionary),
            card,
            meaning,
            synonyms: Vec::new(),
            usage: String::new(),
        }
    }

    pub(crate) fn written(card: Card, back: Option<Written>) -> Self {
        let back = back.unwrap_or_default();
        Self {
            by: said(&back.meaning, MeaningBy::Ai),
            card,
            meaning: back.meaning,
            synonyms: back.synonyms,
            usage: back.usage,
        }
    }
}

/// `by`, when `meaning` says something.
pub(crate) fn said(meaning: &str, by: MeaningBy) -> Option<MeaningBy> {
    (!meaning.trim().is_empty()).then_some(by)
}

/// Who writes a deck's backs, and the backs written before.
#[derive(Clone)]
pub struct Writer {
    pub cards: Arc<dyn CardWriter>,
    pub backs: Backs,
}

type Spot = (Key, u64, String);

/// Backs the AI wrote this session, shared by every pane's decks.
#[derive(Clone, Default)]
pub struct Backs(Arc<Mutex<HashMap<Spot, Written>>>);

impl Backs {
    /// The back `id`'s writer wrote for `wanted`, if any.
    pub fn get(&self, id: &str, wanted: &Wanted) -> Option<Written> {
        let kept = self.0.lock().ok()?;
        kept.get(&spot(id, wanted)).cloned()
    }

    pub fn put(&self, id: &str, wanted: &Wanted, back: Written) {
        let Ok(mut kept) = self.0.lock() else {
            return;
        };
        if kept.len() >= KEPT {
            kept.clear();
        }
        kept.insert(spot(id, wanted), back);
    }

    pub fn count(&self) -> usize {
        self.0.lock().map_or(0, |kept| kept.len())
    }
}

fn spot(id: &str, wanted: &Wanted) -> Spot {
    let mut hasher = DefaultHasher::new();
    wanted.sentence.hash(&mut hasher);
    (wanted.key.clone(), hasher.finish(), id.to_owned())
}

/// What the writer is asked for `card`.
pub fn wanted(card: &Card) -> Wanted {
    Wanted {
        key: card.key.clone(),
        form: card.form.clone(),
        sentence: card.sentence.clone(),
    }
}

/// The class a word's dictionary rows name first, short cells or long.
pub fn class_in(entries: &[Entry]) -> Option<Pos> {
    Pos::of_cells(entries.iter().filter_map(|e| e.pos.as_deref()))
}

/// Up to [`DEFINITIONS`] definitions, those of the card's class first.
///
/// Cells read as [`Pos::read`] reads them, short or long.
pub fn meaning_of(entries: &[Entry], pos: Option<Pos>) -> String {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by_key(|e| pos.is_none() || e.pos.as_deref().and_then(Pos::read) != pos);
    let mut out: Vec<&str> = Vec::new();
    for entry in sorted {
        let text = entry.definition.trim();
        if !text.is_empty() && !out.contains(&text) {
            out.push(text);
        }
    }
    out.truncate(DEFINITIONS);
    out.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_meaning_that_says_something_names_who_wrote_it() {
        assert_eq!(said(" lively ", MeaningBy::Ai), Some(MeaningBy::Ai));
        assert_eq!(said("  ", MeaningBy::Dictionary), None);
    }

    #[test]
    fn backs_are_kept_by_card_sentence_and_writer() {
        let backs = Backs::default();
        let brisk = Wanted {
            key: Key::new("brisk", None),
            form: "brisk".into(),
            sentence: "A brisk walk.".into(),
        };
        let back = Written {
            meaning: "lively".into(),
            ..Written::default()
        };
        backs.put("m|English", &brisk, back.clone());
        assert_eq!(backs.get("m|English", &brisk), Some(back));
        assert_eq!(backs.get("m|Burmese", &brisk), None, "another language");
        let elsewhere = Wanted {
            sentence: "A brisk reply.".into(),
            ..brisk.clone()
        };
        assert_eq!(backs.get("m|English", &elsewhere), None, "another sentence");
        assert_eq!(backs.count(), 1);
    }
}
