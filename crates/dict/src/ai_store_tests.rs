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
    assert_eq!(store.word("book", "en").unwrap().len(), 2, "two classes");
    assert_eq!(store.get("book", "adjective", "en").unwrap(), None);
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
