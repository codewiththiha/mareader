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
use mareader_study::mode::{Card, CardWriter, Key, Pos, Wanted, Written};
use mareader_study::{Failure, Why};

/// Definitions one card shows at most.
const DEFINITIONS: usize = 3;
/// Backs kept at most; past it the cache starts over.
const KEPT: usize = 4096;
/// The error card's words when cards come from the AI and none is set.
pub const NO_MODEL: &str = "No AI model is set for study. Choose one in Settings › AI, \
or let the cards come from the dictionary.";

/// A card and its back; the meaning is empty when no one had it.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub card: Card,
    pub meaning: String,
    /// The AI's synonyms and example; the dictionary gives none.
    pub synonyms: Vec<String>,
    pub usage: String,
}

impl Shown {
    pub fn new(card: Card, meaning: String) -> Self {
        Self {
            card,
            meaning,
            synonyms: Vec::new(),
            usage: String::new(),
        }
    }

    pub(crate) fn written(card: Card, back: Option<Written>) -> Self {
        let back = back.unwrap_or_default();
        Self {
            card,
            meaning: back.meaning,
            synonyms: back.synonyms,
            usage: back.usage,
        }
    }
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

/// A writer's failure as the error card says it.
pub fn said(failure: &Failure) -> String {
    let message = match failure.why {
        Why::NotHere => return NO_MODEL.to_owned(),
        Why::Offline => "The AI couldn’t be reached. Check the connection or the server.",
        why => why.message(),
    };
    match &failure.detail {
        Some(detail) => format!("{message} ({detail})"),
        None => message.to_owned(),
    }
}

/// Up to [`DEFINITIONS`] definitions, those of the card's class first.
pub fn meaning_of(entries: &[Entry], pos: Option<Pos>) -> String {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by_key(|e| pos.is_none() || e.pos.as_deref().and_then(class_of) != pos);
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

/// A dictionary's part-of-speech cell as a class: `adj.`, `n`, `verb`.
fn class_of(cell: &str) -> Option<Pos> {
    let cell = cell.trim().to_lowercase();
    if cell.starts_with("adv") {
        Some(Pos::Adverb)
    } else if cell.starts_with("adj") || cell == "a" {
        Some(Pos::Adjective)
    } else if cell.starts_with('n') {
        Some(Pos::Noun)
    } else if cell.starts_with('v') {
        Some(Pos::Verb)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_cells_read_as_classes() {
        assert_eq!(class_of(" Adv. "), Some(Pos::Adverb));
        assert_eq!(class_of("adjective"), Some(Pos::Adjective));
        assert_eq!(class_of("n"), Some(Pos::Noun));
        assert_eq!(class_of("verb"), Some(Pos::Verb));
        assert_eq!(class_of("prep"), None);
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

    #[test]
    fn failures_say_what_to_do() {
        assert_eq!(said(&Why::NotHere.into()), NO_MODEL);
        let offline = said(&Failure::detailed(Why::Offline, "refused"));
        assert!(offline.contains("couldn’t be reached") && offline.ends_with("(refused)"));
    }
}
