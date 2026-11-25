//! AI dictionary meanings on the lookup card: after the packs' rows, first
//! when the reader says so, and English on its own.

use std::path::Path;
use std::sync::Arc;

use egui_kittest::kittest::Queryable as _;
use mareader_dict::{Dictionaries, Query, Saved};
use mareader_engine::command::Command;
use mareader_ui_egui::ui_command::Sent;

use crate::dictionary::{reader, select, shelf};

/// Keeps an AI's "quick" in Burmese and in English in `dir`.
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
    let saved = vec![said("my", "အမြန်"), said("en", "moving fast")];
    dicts.ask(Query::Remember {
        saved,
        replace: false,
    });
    let (_, replies) = dicts.poll();
    assert!(replies.iter().all(|r| r.failed.is_none()));
}

fn answers(h: &crate::dictionary::H) -> Vec<String> {
    h.state().dict().card_answers().unwrap_or_default()
}

#[test]
fn ai_meanings_follow_the_packs_lead_when_asked_and_stand_alone() {
    let dir = shelf();
    remember(dir.path());
    let mut h = reader(dir.path());
    select(&mut h, "quick");
    h.get_by_label("Look up").click();
    h.run();
    assert_eq!(answers(&h), ["မြန်သော", "အမြန်"]);
    h.get_by_label("Your AI dictionary also has:");
    h.get_by_label("English").click();
    h.run();
    assert_eq!(answers(&h), ["moving fast"], "English alone");
    h.get_by_label("Synonyms");
    h.get_by_label("swift");
    h.key_press(egui::Key::Escape);
    h.run();

    let mut s = h.state().settings().clone();
    s.dictionary.ai_first = true;
    let set = Command::SetSettings(Box::new(s));
    h.state_mut().queue(Sent::Engine(set));
    select(&mut h, "quick");
    h.get_by_label("Look up").click();
    h.run();
    assert_eq!(answers(&h), ["အမြန်", "မြန်သော"], "AI first");
}
