//! A deck's words are the very words the book's shading marks, bar names,
//! and its backs are written in requests of the writer's size.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use mareader_cefr::{Levels as Shading, plan};
use mareader_dict::{Reply, Saved};
use mareader_reflow::Typography;
use mareader_study::mode::{Card, CardJob, CardWriter, Practice, Wanted};

use super::*;
use crate::study_mode::Backs;
use crate::study_mode::backs::wanted;

const TEXT: &str = "After the storm the ephemeral badgers stayed, and Alice watched them. \
                    The well-known aftermath came.";

/// Every key the text holds; "aft" and "badge" are words of their own.
const BANDS: &[(&str, u8)] = &[
    ("after", 1),
    ("aft", 6),
    ("the", 1),
    ("storm", 2),
    ("ephemeral", 6),
    ("badgers", 6),
    ("badger", 6),
    ("badge", 4),
    ("stayed", 1),
    ("stay", 1),
    ("and", 1),
    ("alice", 6),
    ("watched", 1),
    ("watch", 1),
    ("them", 1),
    ("well-known", 0),
    ("well", 1),
    ("known", 1),
    ("aftermath", 5),
    ("came", 1),
];

/// The words the shading marks above `level`, as printed.
fn shaded(level: u8) -> Vec<String> {
    let mut levels = Shading::default();
    for (key, band) in BANDS {
        levels.insert(key, *band);
    }
    let planned = plan(TEXT, &levels, level, usize::MAX);
    assert!(planned.misses.is_empty(), "{:?}", planned.misses);
    let chars: Vec<char> = TEXT.chars().collect();
    let form = |s: &mareader_cefr::Span| chars[s.start..s.end].iter().collect();
    planned.hard.iter().map(form).collect()
}

/// The words a deck above `level` keeps: each as printed, and its lemma.
fn carded(level: u8) -> Vec<(String, String)> {
    let bytes = mareader_pdf::sample::pdf(1, "Sample");
    let opened = Document::open(Arc::new(bytes), &Typography::default());
    let ask = Ask {
        document: Arc::new(opened.unwrap()),
        pages: Vec::new(),
        level,
        max: 10,
        known: Known::default(),
        to: None,
        via: Via::Dictionary,
        writer: None,
    };
    let mut job = DeckJob::new(Arc::new(|| {}), ask);
    let page = Page {
        page: 0,
        chapter: None,
        text: TEXT.to_owned(),
    };
    job.take_pages(&[page]);
    job.bands = BANDS.iter().map(|(k, b)| ((*k).to_owned(), *b)).collect();
    job.find_hard();
    let pair = |h: &Hard| (job.read[h.read].form.clone(), h.lemma.clone());
    job.hard.iter().map(pair).collect()
}

#[test]
fn the_deck_takes_the_words_the_shading_marks() {
    for level in 1..=6 {
        let (shaded, carded) = (shaded(level), carded(level));
        for (form, _) in &carded {
            assert!(shaded.contains(form), "{form} unshaded at {level}");
        }
        let left = shaded
            .iter()
            .filter(|f| !carded.iter().any(|(c, _)| c == *f));
        let left: Vec<&String> = left.collect();
        assert!(left.iter().all(|f| *f == "Alice"), "{left:?} at {level}");
    }
    let c1 = carded(5);
    let pairs: Vec<(&str, &str)> = c1.iter().map(|(f, l)| (f.as_str(), l.as_str())).collect();
    assert_eq!(pairs, [("ephemeral", "ephemeral"), ("badgers", "badger")]);
    assert!(carded(6).is_empty(), "nothing stands above C2");
}

#[test]
fn after_makes_no_card_and_never_aft() {
    let all: Vec<(String, String)> = (1..=6).flat_map(carded).collect();
    let fine = |(form, lemma): &(String, String)| form != "After" && lemma != "aft";
    assert!(all.iter().all(fine));
    assert!(!shaded(5).iter().any(|f| f == "After"));
}

/// Writes as [`Practice::Ready`] does, noting each request's size; the
/// request numbered `fail` fails instead.
struct Counting {
    sizes: std::sync::Mutex<Vec<usize>>,
    fail: Option<usize>,
}

impl CardWriter for Counting {
    fn by(&self) -> String {
        "Counting".to_owned()
    }

    fn id(&self) -> String {
        "counting".to_owned()
    }

    fn per_request(&self) -> usize {
        10
    }

    fn language(&self) -> Option<&'static str> {
        Some("English")
    }

    fn write(&self, words: Vec<Wanted>) -> Box<CardJob> {
        let mut sizes = self.sizes.lock().unwrap();
        let practice = match Some(sizes.len()) == self.fail {
            true => Practice::Failing,
            false => Practice::Ready,
        };
        sizes.push(words.len());
        practice.write(words)
    }
}

/// A dictionary that only keeps what the AI dictionary is given, and
/// answers with `kept` when asked what it keeps.
#[derive(Default)]
struct Filed {
    saved: Vec<Saved>,
    kept: Vec<Saved>,
    asked: Vec<String>,
}

impl Meanings for Filed {
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

    fn ask_kept(&mut self, words: Vec<String>, lang: String) -> Option<Ticket> {
        assert_eq!(lang, "en");
        self.asked = words;
        (!self.kept.is_empty()).then_some(Ticket(1))
    }

    fn take_kept(&mut self, _: Ticket) -> Option<Reply> {
        let saved = self.kept.clone();
        Some(Reply {
            saved,
            ..Reply::default()
        })
    }
}

/// An AI deck of `n` cards, ready to be written by `writer`; every other
/// card has a class.
fn ai_deck(n: usize, writer: &Writer) -> DeckJob {
    let bytes = mareader_pdf::sample::pdf(1, "Sample");
    let opened = Document::open(Arc::new(bytes), &Typography::default());
    let ask = Ask {
        document: Arc::new(opened.unwrap()),
        pages: Vec::new(),
        level: 3,
        max: n,
        known: Known::default(),
        to: None,
        via: Via::Ai,
        writer: Some(writer.clone()),
    };
    let mut job = DeckJob::new(Arc::new(|| {}), ask);
    job.deck.cards = (0..n)
        .map(|i| Card {
            key: Key::new(format!("word{i}"), (i % 2 == 0).then_some(Pos::Noun)),
            form: format!("word{i}"),
            sentence: format!("The word{i} here."),
            band: 5,
            count: 1,
            also: Vec::new(),
        })
        .collect();
    job
}

#[test]
fn a_deck_is_written_in_even_requests_and_goes_on_once_all_answer() {
    let counting = Arc::new(Counting {
        sizes: std::sync::Mutex::new(Vec::new()),
        fail: None,
    });
    let writer = Writer {
        cards: counting.clone(),
        backs: Backs::default(),
    };
    let mut job = ai_deck(25, &writer);
    job.ask_backs(&mut Filed::default()).unwrap();
    assert_eq!(*counting.sizes.lock().unwrap(), [9, 8, 8], "25 by 10");
    let mut filed = Filed::default();
    assert_eq!(job.writing(&mut filed), Ok(true));
    assert_eq!(filed.saved.len(), 13, "those with a class are filed");
    assert!(matches!(job.stage, Stage::Settle), "every request answered");
    assert_eq!(job.settling.len(), 25);
    assert!(job.settling.iter().all(|(s, _)| !s.meaning.is_empty()));
    let mut small = ai_deck(13, &writer);
    small.ask_backs(&mut Filed::default()).unwrap();
    assert_eq!(counting.sizes.lock().unwrap().len(), 3, "13 cached already");
}

#[test]
fn a_failed_request_keeps_the_others_backs_for_the_next_try() {
    let counting = Arc::new(Counting {
        sizes: std::sync::Mutex::new(Vec::new()),
        fail: Some(1),
    });
    let writer = Writer {
        cards: counting.clone(),
        backs: Backs::default(),
    };
    let mut job = ai_deck(32, &writer);
    job.ask_backs(&mut Filed::default()).unwrap();
    let written = job.writing(&mut Filed::default());
    assert!(written.is_err(), "one failed request fails the deck");
    assert_eq!(writer.backs.count(), 24, "the other three are kept");
    let mut again = ai_deck(32, &writer);
    again.ask_backs(&mut Filed::default()).unwrap();
    assert_eq!(*counting.sizes.lock().unwrap(), [8, 8, 8, 8, 8]);
    assert_eq!(again.writing(&mut Filed::default()), Ok(true));
}

#[test]
fn half_a_deck_is_shown_while_the_rest_is_written() {
    let counting = Arc::new(Counting {
        sizes: std::sync::Mutex::new(Vec::new()),
        fail: None,
    });
    let writer = Writer {
        cards: counting.clone(),
        backs: Backs::default(),
    };
    let mut job = ai_deck(25, &writer);
    let id = writer.cards.id();
    let back = Written {
        meaning: "a meaning".to_owned(),
        ..Written::default()
    };
    for card in job.deck.cards.iter().take(12) {
        writer.backs.put(&id, &wanted(card), back.clone());
    }
    job.ask_backs(&mut Filed::default()).unwrap();
    assert!(job.early().is_none(), "12 of 25 is under half");
    let mut job = ai_deck(25, &writer);
    writer.backs.put(&id, &wanted(&job.deck.cards[12]), back);
    job.ask_backs(&mut Filed::default()).unwrap();
    let early = job.early().expect("13 of 25");
    assert_eq!(early.cards.len(), 13);
    assert!(job.early().is_none(), "only once");
    assert!(job.handed);
    assert_eq!(job.writing(&mut Filed::default()), Ok(true));
    assert_eq!(job.settling.len(), 25, "the whole deck goes on");
}

#[test]
fn a_card_the_ai_dictionary_keeps_by_lemma_and_class_is_not_written() {
    let counting = Arc::new(Counting {
        sizes: std::sync::Mutex::new(Vec::new()),
        fail: None,
    });
    let writer = Writer {
        cards: counting.clone(),
        backs: Backs::default(),
    };
    let kept = |word: &str, pos: &str| Saved {
        word: word.into(),
        pos: pos.into(),
        lang: "en".into(),
        meaning: format!("kept {word}"),
        synonyms: (0..5).map(|i| format!("s{i}")).collect(),
        usages: vec!["One.".into(), "Two.".into()],
        ..Saved::default()
    };
    let mut filed = Filed {
        kept: vec![kept("Word0", "noun"), kept("word2", "verb")],
        ..Filed::default()
    };
    let mut job = ai_deck(4, &writer);
    job.ask_backs(&mut filed).unwrap();
    assert_eq!(filed.asked, ["word0", "word2"], "only cards with a class");
    assert!(counting.sizes.lock().unwrap().is_empty(), "none yet");
    assert!(job.reusing(&mut filed, 0.0));
    assert_eq!(*counting.sizes.lock().unwrap(), [3], "word2 is a noun here");
    let reused = job.written[0].clone().unwrap();
    assert_eq!(reused.meaning, "kept Word0");
    assert_eq!(reused.synonyms.len(), 3, "as many as a written back");
    assert_eq!(reused.usage, "One.");
    assert_eq!(job.writing(&mut filed), Ok(true));
    assert_eq!(filed.saved.len(), 1, "only word2 was written with a class");
}
