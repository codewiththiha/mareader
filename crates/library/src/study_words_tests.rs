//! Studied words: kept once per word, met per book, gated, traded, erased.

use crate::disk::Folder;
use crate::merge::Merge;
use crate::test_support::{opened, unsent};
use crate::tests::save;
use crate::{
    BookId, Data, DeviceId, Format, Kind, LearnedBy, Library, NewWord, Spot, WordId, WordStatus,
};

const A: DeviceId = DeviceId(1);
const B: DeviceId = DeviceId(2);

fn word(lemma: &str, status: WordStatus) -> NewWord {
    NewWord {
        lemma: lemma.into(),
        pos: Some("noun".into()),
        band: 5,
        form: format!("{lemma}s"),
        context: format!("Two {lemma}s stood by the door."),
        meaning: "a meaning".into(),
        by: LearnedBy::Proactive,
        status,
        tries: 0,
        first_try: false,
    }
}

fn book(lib: &mut Library, path: &str, content: u128) -> BookId {
    lib.record_open(opened(path, content, Format::Epub, None), 1_000)
}

fn status(lib: &Library, lemma: &str) -> Option<WordStatus> {
    lib.word(WordId::of(lemma, Some("noun")))
        .map(|w| w.status.value)
}

#[test]
fn a_word_is_kept_once_and_met_in_each_book() {
    let mut lib = Library::new(A);
    let (one, two) = (book(&mut lib, "/a.epub", 1), book(&mut lib, "/b.epub", 2));
    let id = lib.record_word(one, word("Lintel", WordStatus::Learning), 2_000);
    assert_eq!(id, Some(WordId::of("lintel", Some("noun"))));
    lib.record_word(two, word("lintel", WordStatus::Learning), 2_100);
    assert_eq!(lib.words().count(), 1, "lemmas fold to lower case");
    assert_eq!(lib.sightings(one).len() + lib.sightings(two).len(), 2);
    let id = id.unwrap();
    assert_eq!(lib.sightings_of(id).len(), 2);
    assert_eq!(
        lib.record_word(BookId(9), word("x", WordStatus::Learning), 2_200),
        None
    );
}

#[test]
fn every_sighting_skips_forgotten_words() {
    let mut lib = Library::new(A);
    let (one, two) = (book(&mut lib, "/a.epub", 1), book(&mut lib, "/b.epub", 2));
    lib.record_word(one, word("lintel", WordStatus::Learning), 2_000);
    let gone = lib.record_word(two, word("sconce", WordStatus::Learning), 2_100);
    assert_eq!(lib.every_sighting().count(), 2);
    let ids: Vec<WordId> = gone.into_iter().collect();
    lib.set_words_alive(&ids, false, 3_000);
    let left: Vec<WordId> = lib.every_sighting().map(|s| s.word).collect();
    assert_eq!(left, [WordId::of("lintel", Some("noun"))]);
}

#[test]
fn a_card_never_lowers_a_memorized_word() {
    let mut lib = Library::new(A);
    let one = book(&mut lib, "/a.epub", 1);
    lib.record_word(one, word("lintel", WordStatus::Memorized), 2_000);
    lib.record_word(one, word("lintel", WordStatus::Learning), 2_100);
    assert_eq!(status(&lib, "lintel"), Some(WordStatus::Memorized));
    assert!(lib.is_memorized("lintel", Some("noun")));
    lib.record_word(one, word("lintel", WordStatus::Missed), 2_200);
    assert_eq!(status(&lib, "lintel"), Some(WordStatus::Missed));
    lib.record_word(one, word("lintel", WordStatus::Learning), 2_300);
    assert_eq!(status(&lib, "lintel"), Some(WordStatus::Learning));
}

#[test]
fn status_and_forgetting_change_only_what_differs() {
    let mut lib = Library::new(A);
    let one = book(&mut lib, "/a.epub", 1);
    let id = lib.record_word(one, word("lintel", WordStatus::Learning), 2_000);
    let ids: Vec<WordId> = id.into_iter().collect();
    lib.take_dirty();
    assert_eq!(lib.set_word_status(&ids, WordStatus::Memorized, 3_000), 1);
    assert_eq!(lib.set_word_status(&ids, WordStatus::Memorized, 3_100), 0);
    let kinds: Vec<Kind> = unsent(&mut lib).iter().map(|r| r.kind).collect();
    assert_eq!(kinds, [Kind::Word]);
    assert_eq!(lib.set_words_alive(&ids, false, 3_200), 1);
    assert_eq!(lib.words().count(), 0);
    assert!(!lib.is_memorized("lintel", Some("noun")));
    assert_eq!(lib.set_words_alive(&ids, true, 3_300), 1);
    assert_eq!(status(&lib, "lintel"), Some(WordStatus::Memorized));
}

#[test]
fn the_gate_is_kept_with_the_place_and_travels() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = folder.load(A).library;
    let one = book(&mut lib, "/a.epub", 1);
    let gate = Spot::Blocks {
        from: [0, 0],
        to: [12, 40],
    };
    assert!(lib.set_unlocked(one, Some(gate), 2_000));
    assert!(!lib.set_unlocked(one, Some(gate), 2_100), "unchanged");
    lib.record_word(one, word("lintel", WordStatus::Learning), 2_200);
    save(&folder, &mut lib);
    let mut back = folder.load(A).library;
    assert_eq!(back.unlocked(one), Some(&gate));
    assert_eq!(back.sightings(one).len(), 1);
    let mut other = Library::new(B);
    book(&mut other, "/elsewhere/a.epub", 1);
    back.mark_all();
    let rows = unsent(&mut back);
    assert!(rows.iter().any(|r| r.kind == Kind::Sighting));
    other.absorb(rows, 2_500);
    assert_eq!(other.sightings(one).len(), 1);
    assert_eq!(status(&other, "lintel"), Some(WordStatus::Learning));
}

#[test]
fn erasing_a_book_takes_its_words_and_gate() {
    let mut lib = Library::new(A);
    let one = book(&mut lib, "/a.epub", 1);
    lib.record_word(one, word("lintel", WordStatus::Learning), 2_000);
    let gate = Spot::Page {
        page: 4,
        rect: [0.0; 4],
    };
    lib.set_unlocked(one, Some(gate), 2_100);
    assert_eq!(lib.receipt(&[one]).words, 1);
    lib.remove_books(&[one], Data::Delete, 3_000);
    assert!(lib.sightings(one).is_empty());
    assert_eq!(lib.unlocked(one), None);
    let gone = lib.sightings.values().all(|s| s.context.value.is_empty());
    assert!(gone, "the book's sentences leave too");
    assert_eq!(lib.words().count(), 1, "the word itself is the reader's");
}

#[test]
fn two_machines_meeting_one_word_merge_either_way() {
    let mut a = Library::new(A);
    let mut b = Library::new(B);
    let (one, _) = (book(&mut a, "/a.epub", 1), book(&mut b, "/a.epub", 1));
    let id = a.record_word(one, word("lintel", WordStatus::Learning), 2_000);
    b.record_word(one, word("lintel", WordStatus::Memorized), 2_500);
    let id = id.unwrap();
    let (mut left, mut right) = (a.word(id).cloned().unwrap(), b.word(id).cloned().unwrap());
    let (ours, theirs) = (left.clone(), right.clone());
    left.merge(theirs);
    right.merge(ours);
    assert_eq!(left, right, "commutes");
    assert_eq!(left.status.value, WordStatus::Memorized, "later wins");
    let seen = |lib: &Library| lib.sightings(one).into_iter().next().cloned().unwrap();
    let (mut left, right) = (seen(&a), seen(&b));
    left.merge(right.clone());
    let mut twice = left.clone();
    twice.merge(right);
    assert_eq!(twice, left, "idempotent");
}
