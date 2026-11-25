//! The AI dictionary on the lookup card: one more dictionary, marked
//! "AI", in the language asked for, and a source of its own languages.

use std::path::Path;
use std::sync::Arc;

use egui_kittest::kittest::Queryable as _;
use mareader_dict::{Dictionaries, Query, Saved, language_code};
use mareader_engine::command::Command;
use mareader_ui_egui::ui_command::Sent;

use crate::dictionary::{H, reader, select, shelf};

/// Keeps an AI's "quick" in Burmese, English and Russian in `dir`.
fn remember(dir: &Path) {
    let mut dicts = Dictionaries::default();
    let attached = dicts.attach(dir.to_owned(), Arc::new(|| {}));
    attached.expect("the folder opens");
    dicts.set_inline(true);
    let said = |lang: &str, meaning: &str| Saved {
        word: "quick".into(),
        pos: "adjective".into(),
        lang: lang.into(),
        meaning: meaning.into(),
        synonyms: vec!["swift".into()],
        ..Saved::default()
    };
    let russian = language_code("Russian");
    let saved = vec![
        said("my", "အမြန်"),
        said("en", "moving fast"),
        said(&russian, "быстрый"),
    ];
    dicts.ask(Query::Remember {
        saved,
        replace: false,
    });
    let (_, replies) = dicts.poll();
    assert!(replies.iter().all(|r| r.failed.is_none()));
}

fn answers(h: &H) -> Vec<String> {
    h.state().dict().card_answers().unwrap_or_default()
}

fn look_up(h: &mut H, words: &str) {
    select(h, words);
    h.get_by_label("Look up").click();
    h.run();
}

/// Sets the card's language, or which dictionary leads.
fn set(h: &mut H, lang: Option<&str>, ai_first: bool) {
    let mut s = h.state().settings().clone();
    s.dictionary.default_lang = lang.map(str::to_owned);
    s.dictionary.ai_first = ai_first;
    let set = Command::SetSettings(Box::new(s));
    h.state_mut().queue(Sent::Engine(set));
    h.run();
}

fn marks(h: &H) -> usize {
    h.query_all_by_label("AI").count()
}

#[test]
fn ai_meanings_extend_the_language_asked_and_carry_the_mark() {
    let dir = shelf();
    remember(dir.path());
    let mut h = reader(dir.path());
    look_up(&mut h, "quick");
    assert_eq!(answers(&h), ["မြန်သော", "အမြန်"]);
    let none = h.query_by_label_contains("also has").is_none();
    assert!(none, "no other language offered");
    let pack = marks(&h);
    h.get_by_label("Next entry").click();
    h.run();
    assert_eq!(marks(&h), pack + 1, "the AI entry is marked");
    h.get_by_label("Synonyms");
    h.key_press(egui::Key::Escape);
    h.run();

    set(&mut h, None, true);
    look_up(&mut h, "quick");
    assert_eq!(answers(&h), ["အမြန်", "မြန်သော"], "AI first");
    h.key_press(egui::Key::Escape);
    h.run();

    set(&mut h, Some("en"), false);
    look_up(&mut h, "quick");
    assert_eq!(answers(&h), ["moving fast"], "English alone");
    h.get_by_label("swift");
}

#[test]
fn a_language_only_the_ai_dictionary_keeps_is_a_dictionary_language() {
    let dir = shelf();
    remember(dir.path());
    let mut h = reader(dir.path());
    look_up(&mut h, "quick brown");
    h.get_by_label("No entry for “quick brown”.");
    h.get_by_label("Try English");
    h.get_by_label("Try Russian").click();
    h.run();
    assert!(answers(&h).is_empty(), "no entry in Russian");
    let swap = h.query_by_label("English → Russian");
    assert!(swap.is_none(), "packs can't swap it");
    h.key_press(egui::Key::Escape);
    h.run();

    let russian = language_code("Russian");
    set(&mut h, Some(&russian), false);
    look_up(&mut h, "quick");
    assert_eq!(answers(&h), ["быстрый"]);
}

fn found(h: &H, text: &str) -> bool {
    h.query_all_by_label_contains(text).count() > 0
}

#[test]
fn the_dictionary_page_searches_the_ai_dictionary_as_one_more() {
    let dir = shelf();
    remember(dir.path());
    let mut h = reader(dir.path());
    let mut s = h.state().settings().clone();
    s.dictionary.langs = vec![mareader_dict::AI_PACK.into()];
    let set = Command::SetSettings(Box::new(s.clone()));
    h.state_mut().queue(Sent::Engine(set));
    let page = h.state_mut().set_option("dict_page", "quick");
    page.expect("a known option");
    h.run_steps(30);
    assert!(found(&h, "быстрый") && found(&h, "moving fast"));
    assert!(found(&h, "Synonyms") && marks(&h) > 0);

    s.dictionary.pair = true;
    s.dictionary.to = Some(language_code("Russian"));
    let set = Command::SetSettings(Box::new(s));
    h.state_mut().queue(Sent::Engine(set));
    h.run_steps(30);
    assert!(found(&h, "быстрый"), "an AI-only language pairs");
    assert!(!found(&h, "moving fast"), "Russian alone");
}
