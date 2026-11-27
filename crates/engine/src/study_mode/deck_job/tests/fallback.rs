//! A capability change must replan an unshown deck without mixing languages.

use super::*;
use mareader_study::mode::{Written, practice_back};
use mareader_study::pending::{News, Pending};
use mareader_study::{Failure, Why};

struct WriterAt {
    fallback: bool,
}

impl CardWriter for WriterAt {
    fn compatible_snapshot(&self) -> Option<Arc<dyn CardWriter>> {
        (!self.fallback).then(|| Arc::new(Self { fallback: true }) as Arc<dyn CardWriter>)
    }
    fn by(&self) -> String {
        "Apple".into()
    }
    fn id(&self) -> String {
        format!("Apple|{}", self.language().unwrap())
    }
    fn per_request(&self) -> usize {
        10
    }
    fn language(&self) -> Option<&'static str> {
        Some(if self.fallback { "English" } else { "Burmese" })
    }
    fn write(&self, words: Vec<Wanted>) -> Box<CardJob> {
        if self.fallback {
            Box::new(Once(Some(Ok(words
                .iter()
                .map(|w| Some(practice_back(w)))
                .collect()))))
        } else {
            Box::new(Once(Some(Err(Why::Language.into()))))
        }
    }
}

struct Once(Option<Result<Vec<Option<Written>>, Failure>>);
impl Pending<Vec<Option<Written>>> for Once {
    fn news(&mut self) -> Vec<News<Vec<Option<Written>>>> {
        self.0.take().map(News::Done).into_iter().collect()
    }
}

#[derive(Default)]
struct Tracked {
    languages: Vec<String>,
    saved: Vec<Saved>,
}
impl Meanings for Tracked {
    fn languages(&self) -> Vec<String> {
        Vec::new()
    }
    fn ask_words(&mut self, _: Vec<String>, _: Vec<String>, _: bool) -> Ticket {
        Ticket(0)
    }
    fn take_words(&mut self, _: Ticket) -> Option<Reply> {
        None
    }
    fn remember(&mut self, saved: Vec<Saved>) {
        self.saved.extend(saved);
    }
    fn ask_kept(&mut self, _: Vec<String>, language: String) -> Option<Ticket> {
        self.languages.push(language);
        None
    }
}

#[test]
fn an_unhanded_deck_reuses_only_the_new_languages_cache_and_filing() {
    let writer = Writer {
        cards: Arc::new(WriterAt { fallback: false }),
        backs: Backs::default(),
    };
    let mut deck = ai_deck(4, &writer);
    let word = wanted(&deck.deck.cards[0]);
    let old = Written {
        meaning: "old Burmese".into(),
        ..Written::default()
    };
    writer.backs.put("Apple|Burmese", &word, old.clone());
    let mut filed = Tracked::default();
    deck.ask_backs(&mut filed).unwrap();
    assert_eq!(deck.written[0], Some(old.clone()));
    assert_eq!(deck.writing(&mut filed), Ok(true), "replanned");
    assert_eq!(filed.languages, ["my", "en"]);
    assert!(
        deck.written.iter().all(Option::is_none),
        "old backs discarded"
    );
    assert_eq!(deck.writing(&mut filed), Ok(true));
    assert_eq!(writer.backs.get("Apple|Burmese", &word), Some(old));
    assert_ne!(
        writer.backs.get("Apple|English", &word).unwrap().meaning,
        "old Burmese"
    );
    assert!(filed.saved.iter().all(|s| s.lang == "en"));
    assert_eq!(filed.saved.len(), 2);
}

#[test]
fn a_deck_already_handed_to_the_reader_is_not_silently_retargeted() {
    let writer = Writer {
        cards: Arc::new(WriterAt { fallback: false }),
        backs: Backs::default(),
    };
    let mut deck = ai_deck(4, &writer);
    let mut filed = Tracked::default();
    deck.ask_backs(&mut filed).unwrap();
    deck.handed = true;
    assert!(deck.writing(&mut filed).is_err());
    assert_eq!(
        deck.writer.as_ref().unwrap().cards.language(),
        Some("Burmese")
    );
    assert_eq!(filed.languages, ["my"]);
}
