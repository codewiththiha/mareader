//! The AI dictionary file: keys, writes and reads.

#![allow(clippy::unwrap_used)]

use super::*;

fn saved(word: &str, pos: &str, lang: &str, meaning: &str) -> Saved {
    Saved {
        word: word.into(),
        pos: pos.into(),
        lang: lang.into(),
        meaning: meaning.into(),
        synonyms: vec!["one".into(), "two".into()],
        usages: vec!["A sentence with the word.".into()],
        model: "Model".into(),
        added: 5,
    }
}

#[test]
fn a_meaning_is_kept_by_word_class_and_language() {
    let dir = tempfile::tempdir().unwrap();
    let store = AiStore::open(&file(dir.path())).unwrap();
    let noun = saved("Book", "noun", "en", "pages bound together");
    let verb = saved("book", "verb", "en", "reserve");
    let japanese = saved("book", "noun", "ja", "本");
    for one in [&noun, &verb, &japanese] {
        assert!(store.put(one, false).unwrap());
    }
    let got = store.get("BOOK", " Noun ", "en").unwrap();
    assert_eq!(got.as_ref(), Some(&noun), "folded word, trimmed class");
    assert_eq!(store.word("book", Some("en")).unwrap().len(), 2, "two");
    assert_eq!(store.word("book", None).unwrap().len(), 3, "any language");
    assert_eq!(store.get("book", "adjective", "en").unwrap(), None);
    assert_eq!(store.langs().unwrap(), ["en", "ja"], "each language once");
    assert_eq!(langs_in(dir.path()), ["en", "ja"]);
    assert!(langs_in(&dir.path().join("none")).is_empty());
}

#[test]
fn a_kept_meaning_stays_unless_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = file(dir.path());
    let store = AiStore::open(&path).unwrap();
    let first = saved("basin", "noun", "en", "a bowl");
    let second = saved("basin", "noun", "en", "land a river drains");
    assert!(store.put(&first, false).unwrap());
    assert!(!store.put(&second, false).unwrap(), "the first stays");
    let kept = store.get("basin", "noun", "en").unwrap();
    assert_eq!(kept, Some(first.clone()));
    assert!(store.put(&second, true).unwrap());
    drop(store);
    let again = AiStore::open(&path).unwrap();
    let kept = again.get("basin", "noun", "en").unwrap().unwrap();
    assert!(kept.same_as(&second), "replaced, and kept on disk");
    assert!(!kept.same_as(&first));
}

#[test]
fn lists_survive_their_column() {
    let dir = tempfile::tempdir().unwrap();
    let store = AiStore::open(&file(dir.path())).unwrap();
    let mut odd = saved("brisk", "adjective", "en", "quick");
    odd.synonyms = vec![format!("a{SEP}b"), String::new(), "lively".into()];
    odd.usages = Vec::new();
    store.put(&odd, false).unwrap();
    let got = store.get("brisk", "adjective", "en").unwrap().unwrap();
    assert_eq!(got.synonyms, ["a b", "lively"]);
    assert!(got.usages.is_empty());
}

#[test]
fn names_kept_before_codes_move_to_their_code() {
    let dir = tempfile::tempdir().unwrap();
    let path = file(dir.path());
    let store = AiStore::open(&path).unwrap();
    let simplified = saved("tea", "noun", "chinese (simplified)", "茶");
    let traditional = saved("tea", "noun", "chinese (traditional)", "茶。");
    let other = saved("cup", "noun", "chinese (traditional)", "杯");
    let russian = saved("cup", "noun", "russian", "чашка");
    for one in [&simplified, &traditional, &other, &russian] {
        assert!(store.put(one, false).unwrap());
    }
    drop(store);
    let store = AiStore::open(&path).unwrap();
    assert_eq!(store.langs().unwrap(), ["ru", "zh"]);
    let tea = store.word("tea", Some("zh")).unwrap();
    assert_eq!(tea.len(), 1, "one entry under one key");
    let cup = store.word("cup", Some("zh")).unwrap();
    assert_eq!(cup.len(), 1);
}

#[test]
fn an_edit_moves_a_meaning_and_asks_before_taking_a_kept_key() {
    let dir = tempfile::tempdir().unwrap();
    let store = AiStore::open(&file(dir.path())).unwrap();
    let noun = saved("book", "noun", "en", "pages bound together");
    let verb = saved("book", "verb", "en", "reserve");
    store.put(&noun, false).unwrap();
    store.put(&verb, false).unwrap();
    let mut said = noun.clone();
    said.meaning = "a bound set of pages".into();
    assert_eq!(store.edit(&noun, &said, false).unwrap(), None, "same key");
    assert_eq!(store.get("book", "noun", "en").unwrap(), Some(said.clone()));
    let mut moved = said.clone();
    moved.pos = "verb".into();
    let kept = store.edit(&said, &moved, false).unwrap();
    assert_eq!(kept, Some(verb), "the verb is kept: ask first");
    assert!(store.get("book", "noun", "en").unwrap().is_some());
    assert_eq!(store.edit(&said, &moved, true).unwrap(), None);
    assert_eq!(store.word("book", None).unwrap(), [moved.clone()]);
    let mut russian = moved.clone();
    russian.lang = "ru".into();
    store.edit(&moved, &russian, false).unwrap();
    assert_eq!(store.langs().unwrap(), ["ru"], "the language moves");
    assert!(store.forget(&russian).unwrap());
    assert!(!store.forget(&russian).unwrap(), "already gone");
    assert!(store.langs().unwrap().is_empty());
}

#[test]
fn a_language_clears_alone() {
    let dir = tempfile::tempdir().unwrap();
    let store = AiStore::open(&file(dir.path())).unwrap();
    for (word, lang) in [("tea", "vi"), ("cup", "vi"), ("tea", "my")] {
        let one = saved(word, "noun", lang, "a drink");
        store.put(&one, false).unwrap();
    }
    assert_eq!(store.clear("vi").unwrap(), 2);
    assert_eq!(store.langs().unwrap(), ["my"]);
    assert_eq!(store.clear("vi").unwrap(), 0, "already gone");
}

#[test]
fn selected_words_are_removed_across_languages_without_touching_other_classes() {
    let dir = tempfile::tempdir().unwrap();
    let store = AiStore::open(&file(dir.path())).unwrap();
    for one in [saved("book", "noun", "en", "pages"), saved("book", "noun", "ja", "本"), saved("book", "verb", "en", "reserve"), saved("tea", "noun", "en", "drink")] {
        store.put(&one, false).unwrap();
    }
    assert_eq!(store.forget_words(&[("BOOK".into(), Some(" noun ".into()))]).unwrap(), 2);
    assert!(store.get("book", "verb", "en").unwrap().is_some());
    assert!(store.get("tea", "noun", "en").unwrap().is_some());
    assert_eq!(store.forget_words(&[("book".into(), None)]).unwrap(), 1);
}
