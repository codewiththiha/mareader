//! The service over fixture packs: cards, bridges, searches, start.
#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mareader_dict::{
    Dictionaries, PACKS, Pack, Pair, Query, Reply, Saved, State, WordMatch, build, db_file,
};

fn pack(id: &str) -> &'static Pack {
    PACKS.iter().find(|p| p.id == id).unwrap()
}

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A folder with the Burmese and French packs converted.
fn shelf() -> (tempfile::TempDir, Dictionaries) {
    let dir = tempfile::tempdir().unwrap();
    build(
        &fixture("en-my.parquet"),
        &db_file(dir.path(), pack("mcfnlp-en-my")),
    )
    .unwrap();
    build(
        &fixture("en-fr.parquet"),
        &db_file(dir.path(), pack("wiktionary-en-fr")),
    )
    .unwrap();
    let mut dicts = Dictionaries::default();
    dicts
        .attach(dir.path().to_owned(), Arc::new(|| {}))
        .unwrap();
    dicts.set_inline(true);
    (dir, dicts)
}

fn ask(dicts: &mut Dictionaries, query: Query) -> Reply {
    let ticket = dicts.ask(query);
    let (_, mut replies) = dicts.poll();
    assert_eq!(replies.len(), 1);
    let reply = replies.remove(0);
    assert_eq!(reply.ticket, ticket);
    reply
}

fn look_up(word: &str, to: Option<&str>) -> Query {
    Query::LookUp {
        word: word.into(),
        to: to.map(Into::into),
    }
}

/// Polls until `done` holds, or fails after a few seconds.
fn wait(dicts: &mut Dictionaries, done: impl Fn(&Dictionaries) -> bool) {
    let start = Instant::now();
    while !done(dicts) {
        assert!(start.elapsed() < Duration::from_secs(20), "timed out");
        dicts.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn converted_packs_are_ready_at_start() {
    let (_dir, dicts) = shelf();
    assert!(dicts.attached());
    let ready: Vec<_> = dicts.ready().iter().map(|p| p.id).collect();
    assert_eq!(ready, ["mcfnlp-en-my", "wiktionary-en-fr"]);
    assert_eq!(*dicts.state(pack("jmdict-en-jp")), State::Absent);
}

#[test]
fn a_card_answers_in_the_first_other_language_without_repeats() {
    let (_dir, mut dicts) = shelf();
    let reply = ask(&mut dicts, look_up("Run", None));
    assert_eq!(
        (reply.from.as_deref(), reply.to.as_deref()),
        (Some("en"), Some("my"))
    );
    let answers: Vec<_> = reply
        .entries
        .iter()
        .map(|e| (e.pos.as_deref(), e.answer()))
        .collect();
    assert_eq!(answers, [(Some("v"), "ပြေးသည်"), (Some("n"), "ပြေးခြင်း")]);
    assert_eq!(reply.ready, 2);
}

#[test]
fn a_card_can_answer_in_a_chosen_language() {
    let (_dir, mut dicts) = shelf();
    let reply = ask(&mut dicts, look_up("light", Some("fr")));
    let answers: Vec<_> = reply.entries.iter().map(|e| e.answer()).collect();
    assert_eq!(answers, ["lumière", "clair"]);
    assert!(reply.entries.iter().all(|e| e.fit == WordMatch::Exact));
}

#[test]
fn burmese_reaches_french_through_english() {
    let (_dir, mut dicts) = shelf();
    let reply = ask(&mut dicts, look_up("အလင်း", Some("fr")));
    assert_eq!(reply.from.as_deref(), Some("my"));
    let answers: Vec<_> = reply
        .entries
        .iter()
        .map(|e| (e.answer(), e.via.as_deref()))
        .collect();
    assert_eq!(
        answers,
        [("lumière", Some("light")), ("clair", Some("light"))]
    );
}

#[test]
fn a_near_miss_is_marked_as_one() {
    let (_dir, mut dicts) = shelf();
    let reply = ask(&mut dicts, look_up("colur", Some("my")));
    assert_eq!(reply.entries.len(), 1);
    assert_eq!(reply.entries[0].word, "colour");
    assert_eq!(reply.entries[0].fit, WordMatch::Fuzzy);
}

#[test]
fn a_search_from_french_answers_with_the_headword() {
    let (_dir, mut dicts) = shelf();
    let pair = Pair {
        from: Some("fr".into()),
        to: Some("en".into()),
    };
    let query = Query::Search {
        text: "clair".into(),
        pair: Some(pair),
        pack: None,
    };
    let reply = ask(&mut dicts, query);
    assert_eq!(reply.entries.len(), 1);
    assert!(reply.entries[0].flipped);
    assert_eq!(reply.entries[0].answer(), "light");
}

#[test]
fn a_search_without_a_pair_spans_packs_or_keeps_to_one() {
    let (_dir, mut dicts) = shelf();
    let all = Query::Search {
        text: "ight".into(),
        pair: None,
        pack: None,
    };
    let reply = ask(&mut dicts, all);
    assert_eq!(reply.entries.len(), 4);
    let one = Query::Search {
        text: "ight".into(),
        pair: None,
        pack: Some("wiktionary-en-fr".into()),
    };
    let reply = ask(&mut dicts, one);
    assert!(reply.entries.iter().all(|e| e.pack == "wiktionary-en-fr"));
    assert_eq!(reply.entries.len(), 2);
    let blank = Query::Search {
        text: "  ".into(),
        pair: None,
        pack: None,
    };
    assert!(ask(&mut dicts, blank).entries.is_empty());
}

#[test]
fn asks_on_the_worker_come_back_by_ticket() {
    let (_dir, mut dicts) = shelf();
    dicts.set_inline(false);
    let ticket = dicts.ask(look_up("quick", Some("fr")));
    let start = Instant::now();
    let reply = loop {
        assert!(start.elapsed() < Duration::from_secs(20), "timed out");
        if let Some(r) = dicts.poll().1.pop() {
            break r;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(reply.ticket, ticket);
    assert_eq!(reply.entries[0].answer(), "rapide");
}

#[test]
fn removing_a_pack_deletes_it() {
    let (dir, mut dicts) = shelf();
    let fr = pack("wiktionary-en-fr");
    dicts.remove(fr);
    assert_eq!(*dicts.state(fr), State::Absent);
    let db = db_file(dir.path(), fr);
    let start = Instant::now();
    while db.exists() {
        assert!(start.elapsed() < Duration::from_secs(20), "timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
    let reply = ask(&mut dicts, look_up("light", Some("fr")));
    assert_eq!(reply.to.as_deref(), Some("my"));
}

#[test]
fn a_whole_download_converts_and_a_partial_one_waits() {
    let dir = tempfile::tempdir().unwrap();
    let fr = pack("wiktionary-en-fr");
    let my = pack("mcfnlp-en-my");
    let part = |p: &Pack| dir.path().join(format!("{}.parquet.part", p.id));
    std::fs::copy(fixture("en-fr.parquet"), part(fr)).unwrap();
    std::fs::write(part(my), b"PAR1 and then nothing").unwrap();
    let mut dicts = Dictionaries::default();
    dicts
        .attach(dir.path().to_owned(), Arc::new(|| {}))
        .unwrap();
    assert_eq!(
        *dicts.state(my),
        State::Paused {
            got: 21,
            total: None
        }
    );
    wait(&mut dicts, |d| *d.state(fr) != State::Converting);
    assert_eq!(*dicts.state(fr), State::Ready);
    assert!(!part(fr).exists());
}

#[test]
fn unattached_dictionaries_answer_nothing() {
    let mut dicts = Dictionaries::default();
    assert!(!dicts.attached());
    assert!(dicts.ready().is_empty());
    let reply = ask(&mut dicts, look_up("run", None));
    assert!(reply.entries.is_empty());
    assert_eq!(reply.ready, 0);
}

#[test]
fn a_word_list_asks_each_word_in_each_language_in_order() {
    let (_dir, mut dicts) = shelf();
    let query = Query::Words {
        words: vec!["light".into(), "runs".into()],
        to: vec!["fr".into(), "xx".into(), "my".into()],
        near: true,
    };
    let reply = ask(&mut dicts, query);
    let asked: Vec<_> = (reply.found.iter())
        .map(|f| (f.word.as_str(), f.to.as_str()))
        .collect();
    let want = [
        ("light", "fr"),
        ("runs", "fr"),
        ("light", "my"),
        ("runs", "my"),
    ];
    assert_eq!(asked, want, "a language without a pack is skipped");
    let light = reply.found.first().unwrap();
    let answers: Vec<_> = light.own.iter().map(|e| e.answer()).collect();
    assert_eq!(answers, ["lumière", "clair"]);
    let runs = reply.found.get(3).unwrap();
    assert!(runs.own.is_empty(), "no row for the word itself");
    assert!(runs.near.iter().any(|e| e.word == "run"), "{:?}", runs.near);
}

#[test]
fn a_word_list_without_near_keeps_to_the_word() {
    let (_dir, mut dicts) = shelf();
    let query = Query::Words {
        words: vec!["runs".into()],
        to: vec!["my".into()],
        near: false,
    };
    let reply = ask(&mut dicts, query);
    let found = reply.found.first().unwrap();
    assert!(found.own.is_empty() && found.near.is_empty());
}

fn remember(saved: Vec<Saved>, replace: bool) -> Query {
    Query::Remember { saved, replace }
}

fn meaning(pos: &str, lang: &str, said: &str) -> Saved {
    Saved {
        word: "book".into(),
        pos: pos.into(),
        lang: lang.into(),
        meaning: said.into(),
        ..Saved::default()
    }
}

#[test]
fn ai_meanings_are_kept_on_disk_and_asked_by_word() {
    let (dir, mut dicts) = shelf();
    let noun = meaning("noun", "en", "pages bound together");
    let verb = meaning("verb", "en", "reserve a place");
    let saved = vec![noun.clone(), verb];
    let reply = ask(&mut dicts, remember(saved, false));
    assert!(reply.failed.is_none() && reply.saved.is_empty());
    assert!(dir.path().join("ai.sqlite").is_file());
    let other = meaning("noun", "en", "a list of bets");
    let saved = vec![other.clone(), noun.clone()];
    let reply = ask(&mut dicts, remember(saved, false));
    assert_eq!(reply.saved, [noun], "only the differing one stayed");
    let words = vec!["Book".to_owned()];
    let lang = "en".to_owned();
    let query = Query::Remembered { words, lang };
    let reply = ask(&mut dicts, query.clone());
    let mut said: Vec<&str> = reply.saved.iter().map(|s| s.meaning.as_str()).collect();
    said.sort_unstable();
    assert_eq!(said, ["pages bound together", "reserve a place"]);
    ask(&mut dicts, remember(vec![other], true));
    let reply = ask(&mut dicts, query);
    let nouns = reply.saved.iter().filter(|s| s.pos == "noun");
    let said: Vec<&str> = nouns.map(|s| s.meaning.as_str()).collect();
    assert_eq!(said, ["a list of bets"], "replaced on asking");
    let japanese = Query::Remembered {
        words: vec!["book".into()],
        lang: "ja".into(),
    };
    assert!(ask(&mut dicts, japanese).saved.is_empty());
}

fn run(lang: &str, said: &str) -> Saved {
    let word = "run".to_owned();
    Saved {
        word,
        ..meaning("verb", lang, said)
    }
}

#[test]
fn a_card_shows_ai_meanings_after_the_packs_and_an_english_one_alone() {
    let (_dir, mut dicts) = shelf();
    let saved = vec![run("my", "ပြေး"), run("en", "move fast on foot")];
    ask(&mut dicts, remember(saved, false));
    let reply = ask(&mut dicts, look_up("Run", None));
    let packs: Vec<&str> = reply.entries.iter().map(|e| e.pack).collect();
    let my = "mcfnlp-en-my";
    assert_eq!(packs, [my, my, mareader_dict::AI_PACK]);
    assert!(reply.saved.is_empty(), "no other language offered");
    assert_eq!(dicts.ai_languages(), ["en", "my"]);
    let reply = ask(&mut dicts, look_up("run", Some("en")));
    let said: Vec<&str> = reply.entries.iter().map(|e| e.answer()).collect();
    assert_eq!(said, ["move fast on foot"]);
    assert_eq!(reply.to.as_deref(), Some("en"));
    let ai = reply.entries[0].ai.as_deref();
    assert_eq!(ai.map(|s| s.pos.as_str()), Some("verb"));
}

#[test]
fn a_language_only_the_ai_dictionary_keeps_is_asked_like_a_packs() {
    let (dir, mut dicts) = shelf();
    ask(&mut dicts, remember(vec![run("ru", "бежать")], false));
    assert_eq!(dicts.ai_languages(), ["ru"]);
    let reply = ask(&mut dicts, look_up("run", Some("ru")));
    let said: Vec<&str> = reply.entries.iter().map(|e| e.answer()).collect();
    assert_eq!(said, ["бежать"]);
    assert_eq!(reply.ready, 3, "the AI dictionary counts");
    let mut again = Dictionaries::default();
    let attached = again.attach(dir.path().to_owned(), Arc::new(|| {}));
    attached.unwrap();
    assert_eq!(again.ai_languages(), ["ru"], "read at start");
    let reply = ask(&mut dicts, look_up("walk", Some("ru")));
    assert_eq!(reply.to.as_deref(), Some("ru"), "kept, with no entry");
    assert!(reply.entries.is_empty());
    let reply = ask(&mut dicts, look_up("walk", Some("de")));
    assert_eq!(reply.to.as_deref(), Some("my"), "a language none has");
}
