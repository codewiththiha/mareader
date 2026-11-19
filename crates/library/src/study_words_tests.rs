use super::*;
use crate::Format;
use crate::library::Library;
use crate::test_support::opened;

fn word(lemma: &str, status: WordStatus) -> NewStudyWord {
    NewStudyWord {
        lemma: lemma.into(),
        class: "noun".into(),
        band: 5,
        meaning: "m".into(),
        context: "c".into(),
        spot: Spot::Page {
            page: 0,
            rect: [0.0; 4],
        },
        by: LearnedBy::Card,
        status,
        attempts: 0,
        first_try: false,
    }
}

#[test]
fn a_word_is_one_record_per_book_and_memorized_is_global() {
    let mut lib = Library::new(crate::DeviceId(7));
    let book = lib.record_open(opened("/a.epub", 1, Format::Epub, None), 10);
    let first = lib.add_study_word(book, word("dignity", WordStatus::Learning), 20);
    let again = lib.add_study_word(book, word("dignity", WordStatus::Memorized), 30);
    assert_eq!(first, again);
    assert_eq!(lib.study_words(Some(book)).len(), 1);
    assert_eq!(lib.memorized_keys(), ["dignity|noun"]);
    let id = first.unwrap_or(StudyWordId(0));
    assert!(lib.set_study_status(id, WordStatus::Learning, 40));
    assert!(lib.memorized_keys().is_empty());
    assert_eq!(lib.set_study_words_alive(&[id], false, 50), 1);
    assert!(lib.study_words(None).is_empty());
}
