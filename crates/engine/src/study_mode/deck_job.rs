//! One batch's deck, gathered step by step: pages, levels, tags, meanings.
//!
//! The pages are read on a thread (in the frame for a book, as word lists
//! are); every later stage moves only when [`DeckJob::step`] is handed the
//! services that answer it. A tagger that is missing, or stops answering
//! for [`TAG_WAIT`] seconds, leaves the word classes unknown. Dictionary
//! cards are only words a dictionary has, looked up a round at a time
//! until the deck is full; a dictionary silent for [`MEANING_WAIT`] seconds
//! ends the deck with those found. Cards from the AI wait on their writer
//! as long as it takes; the deck's waiting card offers the dictionary once
//! that is slow.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use mareader_cefr::{Ticket as TagTicket, candidates};
use mareader_dict::Ticket;
use mareader_session::Document;
use mareader_settings::{TaskId, no_model};
use mareader_study::mode::{CardJob, Deck, Key, Missing, Pos, Sighting, Via, Wanted, Written};
use mareader_study::mode::{build, lemma};

use super::backs::{Shown, Writer, wanted};
use super::known::Known;
use super::sightings::{self, Read};
use crate::session::Waker;
use crate::word_list::{INLINE, LEVEL_ROUND, Levels, Meanings};
use crate::word_marks::{self, Page};

mod lookup;

/// Words read from one batch at most.
const READ_CAP: usize = 20_000;
/// Hard words tagged at most; the rest keep no class.
const TAG_CAP: usize = 240;
/// Seconds the tagger is waited on.
pub const TAG_WAIT: f64 = 10.0;
/// Seconds the dictionaries are waited on; an evicted ask never answers.
pub const MEANING_WAIT: f64 = 20.0;

/// Word levels with the tagger beside them, which names a word's class.
pub trait Tags: Levels {
    /// A tagger is there to ask.
    fn tagging(&self) -> bool;

    /// Asks for `word`'s Penn tag in `sentence`; none if it can't be asked.
    fn ask_tag(&mut self, word: &str, sentence: &str) -> Option<TagTicket>;

    /// The tag `ticket` got once answered; `Some(None)` means no tag.
    fn take_tag(&mut self, ticket: TagTicket) -> Option<Option<String>>;
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
    pub via: Via,
    /// The AI's writer, when there is one.
    pub writer: Option<Writer>,
}

/// A finished deck: its cards, the words it found, those set aside.
#[derive(Clone, Debug, PartialEq)]
pub struct Built {
    pub cards: Vec<Shown>,
    pub found: usize,
    pub skipped_known: usize,
    pub via: Via,
}

/// What a deck being made is doing, as its waiting card says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Doing {
    /// Reading the pages, their levels and word classes.
    Finding,
    Writing,
    LookingUp,
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
    Levels {
        next: usize,
    },
    Tags {
        asked: Vec<(usize, TagTicket)>,
        since: Option<f64>,
    },
    Meanings {
        ticket: Ticket,
        since: Option<f64>,
    },
    /// The writer's job, and the cards it was asked for, by place.
    Writing {
        job: Box<dyn CardJob>,
        asked: Vec<(usize, Wanted)>,
    },
    Done,
}

/// One deck being made; dropping it abandons its asks.
pub struct DeckJob {
    level: u8,
    max: usize,
    known: Known,
    to: Option<String>,
    via: Via,
    writer: Option<Writer>,
    stage: Stage,
    read: Vec<Read>,
    keys: Vec<String>,
    bands: HashMap<String, u8>,
    hard: Vec<Hard>,
    deck: Deck,
    /// Backs found so far, by card; cached ones come first.
    written: Vec<Option<Written>>,
    /// Dictionary cards with a meaning so far; the cards before `looked`
    /// are looked up, and those before `upto` asked.
    kept: Vec<Shown>,
    looked: usize,
    upto: usize,
    started: Option<f64>,
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
            via: ask.via,
            writer: ask.writer,
            stage: Stage::Reading(rx),
            read: Vec::new(),
            keys: Vec::new(),
            bands: HashMap::new(),
            hard: Vec::new(),
            deck: Deck::default(),
            written: Vec::new(),
            kept: Vec::new(),
            looked: 0,
            upto: 0,
            started: None,
            built: None,
        }
    }

    /// Who writes the cards.
    pub fn via(&self) -> Via {
        self.via
    }

    pub fn doing(&self) -> Doing {
        match self.stage {
            Stage::Writing { .. } => Doing::Writing,
            Stage::Meanings { .. } => Doing::LookingUp,
            _ => Doing::Finding,
        }
    }

    /// The cards written out of the deck's, while the writer works.
    pub fn progress(&self) -> Option<(usize, usize)> {
        match self.stage {
            Stage::Writing { .. } => Some(written(&self.written)),
            _ => None,
        }
    }

    /// Memorized words set aside, once the deck is drawn up.
    pub fn skipped(&self) -> usize {
        self.deck.skipped_known
    }

    /// The writer's name, when the AI writes the cards.
    pub fn by(&self) -> Option<String> {
        self.writer.as_ref().map(|w| w.cards.by())
    }

    /// Seconds since the first step, as the slow offer counts them.
    pub fn waited(&self, now: f64) -> f64 {
        self.started.map_or(0.0, |since| (now - since).max(0.0))
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
        self.started.get_or_insert(now);
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
                Stage::Tags { .. } => self.tags(words, meanings, now)?,
                Stage::Meanings { .. } => self.meanings(meanings, now)?,
                Stage::Writing { .. } => self.writing()?,
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
            let lemma = lemma(&read.norm, listed);
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

    fn tags(
        &mut self,
        tags: &mut dyn Tags,
        meanings: &mut dyn Meanings,
        now: f64,
    ) -> Result<bool, String> {
        let Stage::Tags { asked, since } = &mut self.stage else {
            return Ok(false);
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
            return Ok(false);
        }
        self.build_deck(meanings)?;
        Ok(true)
    }

    fn build_deck(&mut self, meanings: &mut dyn Meanings) -> Result<(), String> {
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
        // The dictionary may lack some words, so it sees every one in turn.
        let max = match self.via {
            Via::Ai => self.max,
            Via::Dictionary => usize::MAX,
        };
        self.deck = build(seen, self.level, |k| known.has(k), max);
        if self.via == Via::Ai {
            return self.ask_backs();
        }
        self.look_up(meanings)
    }

    /// Asks the writer for the backs not cached; with all cached, finishes.
    fn ask_backs(&mut self) -> Result<(), String> {
        let none = || no_model(TaskId::StudyCards);
        let writer = self.writer.clone().ok_or_else(none)?;
        let id = writer.cards.id();
        let mut asked = Vec::new();
        self.written = (self.deck.cards.iter().enumerate())
            .map(|(i, card)| {
                let wanted = wanted(card);
                let cached = writer.backs.get(&id, &wanted);
                if cached.is_none() {
                    asked.push((i, wanted));
                }
                cached
            })
            .collect();
        if asked.is_empty() {
            self.finish_written();
            return Ok(());
        }
        let words = asked.iter().map(|(_, w)| w.clone()).collect();
        let job = writer.cards.write(words);
        self.stage = Stage::Writing { job, asked };
        Ok(())
    }

    /// Takes the writer's answer into the deck and the cache.
    fn writing(&mut self) -> Result<bool, String> {
        let Stage::Writing { job, asked } = &mut self.stage else {
            return Ok(false);
        };
        let Some(answer) = job.take() else {
            return Ok(false);
        };
        let backs = answer.map_err(|f| f.said())?;
        let asked = std::mem::take(asked);
        if let Some(writer) = &self.writer {
            let id = writer.cards.id();
            for ((i, wanted), back) in asked.into_iter().zip(backs) {
                let Some(back) = back else {
                    continue;
                };
                writer.backs.put(&id, &wanted, back.clone());
                if let Some(slot) = self.written.get_mut(i) {
                    *slot = Some(back);
                }
            }
        }
        self.finish_written();
        Ok(true)
    }

    fn finish_written(&mut self) {
        let deck = std::mem::take(&mut self.deck);
        let mut backs = std::mem::take(&mut self.written).into_iter();
        let shown = deck.cards.into_iter();
        let shown = shown.map(|card| Shown::written(card, backs.next().flatten()));
        self.done(shown.collect(), deck.found, deck.skipped_known);
    }

    fn done(&mut self, cards: Vec<Shown>, found: usize, skipped_known: usize) {
        self.built = Some(Built {
            cards,
            found,
            skipped_known,
            via: self.via,
        });
        self.stage = Stage::Done;
    }
}

/// The backs found so far, cached ones first, and all there are to find.
fn written(backs: &[Option<Written>]) -> (usize, usize) {
    (backs.iter().flatten().count(), backs.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_counts_the_backs_found() {
        let back = Written::default();
        assert_eq!(written(&[]), (0, 0));
        assert_eq!(written(&[Some(back.clone()), None, Some(back)]), (2, 3));
    }
}
