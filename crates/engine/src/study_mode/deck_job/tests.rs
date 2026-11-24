//! A deck's words are the very words the book's shading marks, bar names.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use mareader_cefr::{Levels as Shading, plan};
use mareader_reflow::Typography;

use super::*;

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
