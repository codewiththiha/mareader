//! One batch's deck, gathered step by step: pages, levels, tags, meanings.
//!
//! The pages are read on a thread (in the frame for a book, as word lists
//! are); every later stage moves only when [`DeckJob::step`] is handed the
//! services that answer it. A tagger that is missing, or stops answering
//! for [`TAG_WAIT`] seconds, leaves the word classes unknown; dictionaries
//! silent for [`MEANING_WAIT`] seconds leave the meanings empty.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use mareader_cefr::{Ticket as TagTicket, candidates};
use mareader_dict::{Entry, Ticket};
use mareader_session::Document;
use mareader_study::mode::{Card, Deck, Key, Missing, Pos, Sighting, build, lemma};

use super::sightings::{self, Read};
use crate::session::Waker;
use crate::word_list::{INLINE, LEVEL_ROUND, Levels, Meanings};
use crate::word_marks::{self, Page};

/// Words read from one batch at most.
const READ_CAP: usize = 20_000;
/// Hard words tagged at most; the rest keep no class.
const TAG_CAP: usize = 240;
/// Seconds the tagger is waited on.
pub const TAG_WAIT: f64 = 10.0;
/// Seconds the dictionaries are waited on; an evicted ask never answers.
pub const MEANING_WAIT: f64 = 20.0;
/// Definitions one card shows at most.
const DEFINITIONS: usize = 3;

/// Word levels with the tagger beside them, which names a word's class.
pub trait Tags: Levels {
    /// A tagger is there to ask.
    fn tagging(&self) -> bool;

    /// Asks for `word`'s Penn tag in `sentence`; none if it can't be asked.
    fn ask_tag(&mut self, word: &str, sentence: &str) -> Option<TagTicket>;

    /// The tag `ticket` got once answered; `Some(None)` means no tag.
    fn take_tag(&mut self, ticket: TagTicket) -> Option<Option<String>>;
}

/// Words already memorized; a key without a class matches every class.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Known {
    exact: HashSet<Key>,
    lemmas: HashSet<String>,
}

impl Known {
    pub fn of(keys: impl IntoIterator<Item = Key>) -> Self {
        let mut known = Self::default();
        for key in keys {
            known.add(key);
        }
        known
    }

    pub fn add(&mut self, key: Key) {
        self.lemmas.insert(key.lemma.clone());
        self.exact.insert(key);
    }

    pub fn has(&self, key: &Key) -> bool {
        let bare = || self.exact.contains(&Key::new(&*key.lemma, None));
        match key.pos {
            Some(_) => self.exact.contains(key) || bare(),
            None => self.lemmas.contains(&key.lemma),
        }
    }
}

/// What one deck is made of, and from where.
pub struct Ask {
    pub document: Arc<Document>,
    /// The batch's pages, as the pane counts them.
    pub pages: Vec<usize>,
    pub level: u8,
    pub max: usize,
    pub known: Known,
    /// The dictionary language meanings are asked in.
    pub to: Option<String>,
}

/// A card and the meaning on its back; empty when no dictionary had it.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub card: Card,
    pub meaning: String,
}

/// A finished deck: its cards, the words it found, those set aside.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Built {
    pub cards: Vec<Shown>,
    pub found: usize,
    pub skipped_known: usize,
}

/// A word above the level, before its class is known.
struct Hard {
    read: usize,
    lemma: String,
    band: u8,
    pos: Option<Pos>,
    /// The tagger read it as a name.
    name: bool,
}

enum Stage {
    Reading(Receiver<Vec<Page>>),
    Levels { next: usize },
    Tags { asked: Vec<(usize, TagTicket)>, since: Option<f64> },
    Meanings { ticket: Ticket, since: Option<f64> },
    Done,
}

/// One deck being made; dropping it abandons its asks.
pub struct DeckJob {
    level: u8,
    max: usize,
    known: Known,
    to: Option<String>,
    stage: Stage,
    read: Vec<Read>,
    keys: Vec<String>,
    bands: HashMap<String, u8>,
    hard: Vec<Hard>,
    deck: Deck,
    built: Option<Built>,
}

impl DeckJob {
    pub fn new(wake: Waker, ask: Ask) -> Self {
        let inline = ask.pages.len() <= INLINE || ask.document.is_reflowable();
        let rx = word_marks::spawn(wake, ask.document, ask.pages, inline);
        Self {
            level: ask.level,
            max: ask.max,
            known: ask.known,
            to: ask.to,
            stage: Stage::Reading(rx),
            read: Vec::new(),
            keys: Vec::new(),
            bands: HashMap::new(),
            hard: Vec::new(),
            deck: Deck::default(),
            built: None,
        }
    }

    /// Moves on as far as answers allow: the deck once built, or why not.
    ///
    /// After it hands the deck over once, it answers `Ok(None)`.
    pub fn step(
        &mut self,
        words: &mut dyn Tags,
        meanings: &mut dyn Meanings,
        now: f64,
    ) -> Result<Option<Built>, String> {
        loop {
            let moved = match &mut self.stage {
                Stage::Reading(rx) => match rx.try_recv() {
                    Ok(pages) => {
                        self.take_pages(&pages);
                        true
                    }
                    Err(TryRecvError::Empty) => false,
                    Err(TryRecvError::Disconnected) => {
                        return Err("Couldn't read those pages".to_owned());
                    }
                },
                Stage::Levels { .. } => self.levels(words)?,
                Stage::Tags { .. } => self.tags(words, meanings, now),
                Stage::Meanings { .. } => self.meanings(meanings, now),
                Stage::Done => return Ok(self.built.take()),
            };
            if !moved {
                return Ok(None);
            }
        }
    }

    fn take_pages(&mut self, pages: &[Page]) {
        let read = pages.iter().flat_map(|p| sightings::words(&p.text));
        self.read = read.take(READ_CAP).collect();
        let mut seen = HashSet::new();
        let keys = self.read.iter().flat_map(Read::keys);
        self.keys = keys.filter(|k| seen.insert(k.clone())).collect();
        self.stage = Stage::Levels { next: 0 };
    }

    fn levels(&mut self, words: &mut dyn Tags) -> Result<bool, String> {
        let Stage::Levels { next } = &mut self.stage else {
            return Ok(false);
        };
        if words.coming() {
            return Ok(false);
        }
        if !words.ready() {
            return Err(Missing::Levels.reason().to_owned());
        }
        let start = *next;
        let round = self.keys.get(start..).unwrap_or_default();
        let end = LEVEL_ROUND.min(round.len());
        let round = round.get(..end).unwrap_or_default();
        if round.is_empty() {
            self.find_hard();
            self.ask_tags(words);
            return Ok(true);
        }
        let mut all = true;
        for (key, band) in round.iter().zip(words.bands(round)) {
            match band {
                Some(b) => {
                    self.bands.insert(key.clone(), b);
                }
                None => all = false,
            }
        }
        if all {
            *next = start.saturating_add(round.len());
        }
        Ok(all)
    }

    /// Each word's lemma and band; those above the level are kept.
    fn find_hard(&mut self) {
        let bands = &self.bands;
        let listed = |k: &str| bands.get(k).is_some_and(|b| *b > 0);
        let mut hard = Vec::new();
        for (i, read) in self.read.iter().enumerate() {
            let lemma = lemma(&read.norm, &listed);
            let own = bands.get(&lemma).copied().filter(|b| *b > 0);
            let parts = || {
                let found = candidates(&read.form).into_iter();
                found.filter_map(|k| bands.get(&k).copied()).max()
            };
            let band = own.or_else(parts).unwrap_or(0);
            if band > self.level {
                hard.push(Hard {
                    read: i,
                    lemma,
                    band,
                    pos: None,
                    name: false,
                });
            }
        }
        self.hard = hard;
    }

    fn ask_tags(&mut self, tags: &mut dyn Tags) {
        let mut asked = Vec::new();
        if tags.tagging() {
            for (i, hard) in self.hard.iter().enumerate().take(TAG_CAP) {
                let Some(read) = self.read.get(hard.read) else {
                    continue;
                };
                if let Some(ticket) = tags.ask_tag(&read.form, &read.sentence) {
                    asked.push((i, ticket));
                }
            }
        }
        self.stage = Stage::Tags { asked, since: None };
    }

    fn tags(&mut self, tags: &mut dyn Tags, meanings: &mut dyn Meanings, now: f64) -> bool {
        let Stage::Tags { asked, since } = &mut self.stage else {
            return false;
        };
        let since = *since.get_or_insert(now);
        let mut waiting = Vec::new();
        for (i, ticket) in asked.drain(..) {
            match tags.take_tag(ticket) {
                None => waiting.push((i, ticket)),
                Some(tag) => {
                    if let Some(hard) = self.hard.get_mut(i) {
                        hard.name = tag.as_deref().is_some_and(|t| t.trim().starts_with("NNP"));
                        hard.pos = tag.as_deref().and_then(Pos::of_tag);
                    }
                }
            }
        }
        let gave_up = !tags.tagging() || now - since > TAG_WAIT;
        if !waiting.is_empty() && !gave_up {
            *asked = waiting;
            return false;
        }
        self.build_deck(meanings);
        true
    }

    fn build_deck(&mut self, meanings: &mut dyn Meanings) {
        let read = &self.read;
        let seen = self.hard.iter().filter(|h| !h.name).filter_map(|h| {
            let r = read.get(h.read)?;
            Some(Sighting {
                key: Key::new(&*h.lemma, h.pos),
                form: r.form.clone(),
                band: h.band,
                sentence: r.sentence.clone(),
            })
        });
        let known = &self.known;
        self.deck = build(seen, self.level, |k| known.has(k), self.max);
        let mut words: Vec<String> = Vec::new();
        for card in &self.deck.cards {
            if !words.contains(&card.key.lemma) {
                words.push(card.key.lemma.clone());
            }
        }
        match self.to.clone() {
            Some(to) if !words.is_empty() => {
                let ticket = meanings.ask_words(words, vec![to], false);
                self.stage = Stage::Meanings {
                    ticket,
                    since: None,
                };
            }
            _ => self.finish(&HashMap::new()),
        }
    }

    fn meanings(&mut self, meanings: &mut dyn Meanings, now: f64) -> bool {
        let Stage::Meanings { ticket, since } = &mut self.stage else {
            return false;
        };
        let waited = now - *since.get_or_insert(now);
        match meanings.take_words(*ticket) {
            Some(reply) => {
                let found = reply.found.into_iter().map(|f| (f.word, f.own));
                self.finish(&found.collect());
            }
            None if waited > MEANING_WAIT => self.finish(&HashMap::new()),
            None => return false,
        }
        true
    }

    fn finish(&mut self, found: &HashMap<String, Vec<Entry>>) {
        let deck = std::mem::take(&mut self.deck);
        let shown = deck.cards.into_iter().map(|card| {
            let entries = found.get(&card.key.lemma).map_or(&[][..], Vec::as_slice);
            let meaning = meaning_of(entries, card.key.pos);
            Shown { card, meaning }
        });
        self.built = Some(Built {
            cards: shown.collect(),
            found: deck.found,
            skipped_known: deck.skipped_known,
        });
        self.stage = Stage::Done;
    }
}

/// Up to [`DEFINITIONS`] definitions, those of the card's class first.
fn meaning_of(entries: &[Entry], pos: Option<Pos>) -> String {
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
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_key_without_a_class_matches_every_class() {
        let known = Known::of([Key::new("run", None), Key::new("swim", Some(Pos::Verb))]);
        assert!(known.has(&Key::new("run", Some(Pos::Verb))));
        assert!(known.has(&Key::new("swim", None)));
        assert!(known.has(&Key::new("swim", Some(Pos::Verb))));
        assert!(!known.has(&Key::new("swim", Some(Pos::Noun))));
        assert!(!known.has(&Key::new("fly", None)));
    }

    #[test]
    fn dictionary_cells_read_as_classes() {
        assert_eq!(class_of(" Adv. "), Some(Pos::Adverb));
        assert_eq!(class_of("adjective"), Some(Pos::Adjective));
        assert_eq!(class_of("n"), Some(Pos::Noun));
        assert_eq!(class_of("verb"), Some(Pos::Verb));
        assert_eq!(class_of("prep"), None);
    }
}
