//! The AI dictionary on the lookup card: one more dictionary, marked
//! "AI", in the language asked for, and a source of its own languages.

use std::path::Path;
use std::sync::Arc;

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use mareader_dict::{Dictionaries, Query, Saved, ai_only, language_code};
use mareader_engine::command::Command;
use mareader_ui_egui::dict::ai_edit;
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
    // The entry's pencil, then one on a sense card, itself a button.
    let pencils = h.get_all_by_label(ai_edit::EDIT).count();
    assert!(pencils > 1, "a pencil on each AI sense");
    h.get_all_by_label(ai_edit::EDIT).last().unwrap().click();
    h.run();
    h.get_by_label(ai_edit::TITLE);
    h.key_press(egui::Key::Escape);
    h.run();

    s.dictionary.pair = true;
    s.dictionary.to = Some(language_code("Russian"));
    let set = Command::SetSettings(Box::new(s));
    h.state_mut().queue(Sent::Engine(set));
    h.run_steps(30);
    assert!(found(&h, "быстрый"), "an AI-only language pairs");
    assert!(!found(&h, "moving fast"), "Russian alone");
}

/// Opens the editor on the card's first entry, an AI one.
fn open_editor(h: &mut H) {
    h.get_by_label(ai_edit::EDIT).click();
    h.run();
    h.get_by_label(ai_edit::TITLE);
}

/// Types `text` into the editor's `name` field over what it held.
fn retype(h: &mut H, (role, name): (Role, &str), text: &str) {
    h.get_by_role_and_label(role, name).focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_role_and_label(role, name).type_text(text);
    h.run();
}

#[test]
fn an_ai_entry_edits_and_deletes_from_the_card() {
    let dir = shelf();
    remember(dir.path());
    let mut h = reader(dir.path());
    set(&mut h, None, true);
    look_up(&mut h, "quick");
    assert_eq!(answers(&h), ["အမြန်", "မြန်သော"]);
    open_editor(&mut h);
    let usages = (Role::MultilineTextInput, "Usages");
    retype(&mut h, usages, "Come quick.");
    h.get_by_label(ai_edit::SAVE).click();
    h.run();
    assert!(h.query_by_label(ai_edit::TITLE).is_none(), "saved, closed");
    assert!(found(&h, "Come quick."), "the card shows the edit");

    open_editor(&mut h);
    h.get_by_label(ai_edit::DELETE).click();
    h.run();
    h.get_by_label(ai_edit::DELETE_SURE).click();
    h.run();
    assert!(h.query_by_label(ai_edit::TITLE).is_none());
    assert_eq!(answers(&h), ["မြန်သော"], "the AI entry is gone");
}

#[test]
fn an_edit_onto_a_kept_entry_asks_before_replacing_it() {
    let dir = shelf();
    remember(dir.path());
    let mut h = reader(dir.path());
    set(&mut h, None, true);
    look_up(&mut h, "quick");
    open_editor(&mut h);
    let language = (Role::ComboBox, ai_edit::LANGUAGE);
    h.get_by_role_and_label(language.0, language.1).click();
    h.run();
    h.get_all_by_label("English").last().unwrap().click();
    h.run();
    h.get_by_label(ai_edit::SAVE).click();
    h.run();
    h.get_by_label_contains("already has quick (adjective, English)");
    h.get_by_label(ai_edit::REPLACE).click();
    h.run();
    assert!(h.query_by_label(ai_edit::TITLE).is_none());
    assert_eq!(answers(&h), ["မြန်သော"], "moved out of Burmese");
    h.key_press(egui::Key::Escape);
    h.run();

    set(&mut h, Some("en"), false);
    look_up(&mut h, "quick");
    assert_eq!(answers(&h), ["အမြန်"], "it took the English entry's place");
}

#[test]
fn resting_on_a_word_reads_the_ai_dictionary_with_no_pack() {
    let (words, _) = crate::common::shelves();
    let only_ai = tempfile::tempdir().expect("a folder");
    remember(only_ai.path());
    let mut h = crate::common::levels_reader(words.path(), Some(only_ai.path()));
    h.hover_at(crate::common::spot(&h, "quick"));
    h.run_steps(20);
    assert_eq!(h.state().dict().card_word(), Some("quick"));
    assert!(!answers(&h).is_empty(), "the AI dictionary answers");
}

#[test]
fn settings_clear_one_language_from_the_ai_dictionary() {
    let dir = shelf();
    remember(dir.path());
    let mut h = reader(dir.path());
    let tab = mareader_engine::settings::Tab::Dictionary;
    let open = Sent::Engine(Command::OpenSettings(tab));
    h.state_mut().queue(open);
    h.run();
    h.get_by_label("AI dictionary");
    h.get_by_label("About Lookup card").click();
    h.run();
    h.get_by_label(mareader_ui_egui::settings_window::ELSEWHERE);
    h.get_by_label("About Lookup card").click();
    h.run();
    let clears = h.query_all_by_label_contains("Clear: ");
    let labels = clears.filter_map(|n| n.accesskit_node().label());
    let rows: Vec<String> = labels.collect();
    let names: Vec<String> = rows.iter().map(|l| l.replace("Clear: ", "")).collect();
    assert_eq!(names, ["Burmese", "English", "Russian"], "by name");
    h.get_by_label("Clear: Russian").click();
    h.run();
    h.get_by_label("Keep").click();
    h.run();
    let sure = h.query_by_label("Clear for good: Russian");
    assert!(sure.is_none(), "kept");
    h.get_by_label("Clear: Russian").click();
    h.run();
    h.get_by_label("Clear for good: Russian").click();
    h.run_steps(30);
    let langs = h.state().dict().languages();
    assert!(!langs.iter().any(|l| l == "ru"), "Russian goes");
    assert!(langs.iter().any(|l| l == "en"), "the rest stay");
    assert!(h.query_by_label("Clear: Russian").is_none());
    h.get_by_label("Clear: English");
}

/// Whether an open list offers `label`, not a card's tag.
fn listed(h: &H, label: &str) -> bool {
    let mut all = h.query_all_by_label(label);
    all.any(|n| n.accesskit_node().toggled().is_some())
}

/// Opens the dictionary picker and rests on its AI row.
fn rest_on_ai(h: &mut H) {
    h.get_by_label("Dictionary to search").click();
    h.run();
    h.get_by_label("AI dictionary").hover();
    h.run();
}

#[test]
fn the_ai_dictionary_lists_its_languages_beside_it_and_finds_them() {
    let dir = shelf();
    remember(dir.path());
    let mut h = reader(dir.path());
    let page = h.state_mut().set_option("dict_page", "quick");
    page.expect("a known option");
    h.run_steps(30);
    rest_on_ai(&mut h);
    let both = listed(&h, "All AI words") && listed(&h, "English → Russian");
    assert!(both, "open beside the row");
    h.event(egui::Event::Text("rus".into()));
    h.run();
    let english = listed(&h, "English → English");
    assert!(listed(&h, "English → Russian") && !english, "typing finds");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!listed(&h, "English → Russian"), "Escape folds the list");
    assert!(listed(&h, "Every dictionary"), "the picker stays");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!listed(&h, "Every dictionary"), "then the picker goes");

    // English, Burmese, Russian match: down past the end, back up one.
    rest_on_ai(&mut h);
    h.event(egui::Event::Text("english".into()));
    h.run();
    use egui::Key::{ArrowDown, ArrowUp, Enter};
    for key in [ArrowDown, ArrowDown, ArrowDown, ArrowUp, Enter] {
        h.key_press(key);
        h.run();
    }
    h.run_steps(30);
    let langs = h.state().settings().dictionary.langs.clone();
    assert_eq!(langs, [ai_only("my")], "Enter: the lit match");
    assert!(found(&h, "အမြန်"), "the AI's own row");
    let others = found(&h, "быстрый") || found(&h, "moving fast");
    assert!(!others, "no other language");
    h.get_by_label("Dictionary to search").click();
    h.run();
    h.get_by_label("AI dictionary").click();
    h.run_steps(30);
    let langs = h.state().settings().dictionary.langs.clone();
    assert_eq!(langs, [mareader_dict::AI_PACK], "a click: every language");
    assert!(found(&h, "moving fast") && found(&h, "быстрый"));
}
