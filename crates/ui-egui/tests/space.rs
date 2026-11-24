//! Space, headless: the library bar's way in to the hub, the words page's
//! tabs and their counts, a word memorized or forgotten from its row, and
//! its sentence opening the book there, and a library card's counts.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::library::Action;
use mareader_engine::route::Route;
use mareader_engine::space::{Room, Tally, tally};
use mareader_library::WordStatus;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::space::open_sentence;
use mareader_ui_egui::ui_command::{Sent, UiCommand};

use crate::common::{act, press, reader};

type H = Harness<'static, MareaderApp>;
type Out = Result<(), Box<dyn std::error::Error>>;

/// Three books and the made-up words studied in them, on `page`.
fn shelf(page: &str) -> H {
    reader(&[("books", "3"), ("words", "demo"), ("route", page)])
}

/// The same books on Space's words page.
fn words() -> H {
    reader(&[("books", "3"), ("words", "demo"), ("space", "words")])
}

fn status(h: &H, lemma: &str) -> Option<WordStatus> {
    let lib = h.state().session().library_model();
    let found = lib.words().find(|w| w.lemma == lemma);
    found.map(|w| w.status.value)
}

#[test]
fn the_library_bar_opens_the_hub_and_its_card_every_sighting() {
    let mut h = shelf("library");
    assert_eq!(h.state().route(), Route::Library);
    h.get_by_label("Space").click();
    h.run();
    assert_eq!(h.state().route(), Route::Space);
    assert_eq!(h.state().session().room(), Room::Hub);
    h.get_by_label("Vocabs").click();
    h.run();
    assert_eq!(h.state().session().room(), Room::Words);
    // Two of the eight words were met in a second book.
    assert!(h.query_by_label("All 10").is_some());
    assert!(h.query_by_label("Memorized 4").is_some());
    assert!(h.query_by_label("Missed 2").is_some());
    h.get_by_label("Merged").click();
    h.run();
    assert!(h.query_by_label("All 8").is_some(), "a row per word");
}

#[test]
fn a_tab_keeps_only_its_status() {
    let mut h = words();
    h.get_by_label("Missed 2").click();
    h.run();
    assert!(h.query_by_label("Memorize ponder").is_some());
    assert!(h.query_by_label("Un-memorize arcane").is_none());
    assert!(
        h.query_by_label("All 10").is_some(),
        "counts ignore the tab"
    );
}

#[test]
fn a_row_memorizes_and_forgets_its_word() {
    let mut h = words();
    assert_eq!(status(&h, "ponder"), Some(WordStatus::Missed));
    press(&mut h, "Memorize ponder");
    h.run();
    assert_eq!(status(&h, "ponder"), Some(WordStatus::Memorized));
    assert!(h.query_by_label("Memorized 5").is_some());
    press(&mut h, "Un-memorize ponder");
    h.run();
    assert_eq!(status(&h, "ponder"), Some(WordStatus::Learning));
    press(&mut h, "Forget ponder");
    let at = h.get_by_label("Forget ponder").rect().center();
    let met = h.ctx.layer_id_at(at);
    h.run();
    let why = format!("forgotten words leave Space; {at:?} met {met:?}");
    assert_eq!(status(&h, "ponder"), None, "{why}");
    assert!(h.query_by_label("All 9").is_some());
}

#[test]
fn escape_goes_back_to_the_hub_then_the_library() {
    let mut h = words();
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().route(), Route::Space);
    assert_eq!(h.state().session().room(), Room::Hub);
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().route(), Route::Library);
}

#[test]
fn a_rows_sentence_opens_its_book_and_finds_it_there() -> Out {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let raven = dir.join("../epub/tests/books/the-raven-gutenberg-17192.epub");
    let raven = raven.canonicalize()?;
    let mut h = reader(&[("epub", raven.to_str().ok_or("a path in UTF-8")?)]);
    h.run_steps(3);
    act(&mut h, Action::SeedWords);
    let words = Sent::Engine(Command::ShowSpace(Room::Words));
    h.state_mut().queue(words);
    h.run();
    press(&mut h, "Show where lintel was met");
    h.run();
    press(&mut h, &open_sentence("lintel"));
    for _ in 0..20 {
        h.run_steps(1);
    }
    assert_eq!(h.state().route(), Route::Reader, "back in the book");
    let find = h.state().find();
    assert!(find.is_open(), "the find bar looks for the sentence");
    assert_eq!(find.query(), "ducked beneath the lintel of the low");
    Ok(())
}

#[test]
fn a_library_cards_counts_open_space_on_that_book() -> Out {
    let mut h = shelf("library");
    act(&mut h, Action::SeedMarks);
    h.run();
    let counts = tally(h.state().session().library_model());
    // A book whose word count no other book shares, so its label is its own.
    let words: Vec<usize> = counts.values().map(|n| n.words).collect();
    let alone = |n: &&Tally| n.words > 0 && words.iter().filter(|w| **w == n.words).count() == 1;
    let n = counts.values().find(alone);
    let n = *n.ok_or("a book with its own word count")?;
    h.get_by_label(&format!("{} words", n.words)).click();
    h.run();
    assert_eq!(h.state().route(), Route::Space);
    assert_eq!(h.state().session().room(), Room::Words);
    let all = format!("All {}", n.words);
    assert!(h.query_by_label(&all).is_some(), "that book's alone");
    h.state_mut().queue(Sent::Ui(UiCommand::ShowLibrary));
    h.run();
    let noted = counts.values().find(|n| n.notes > 0);
    let notes = match noted.ok_or("a book with notes")?.notes {
        1 => "1 note".to_owned(),
        n => format!("{n} notes"),
    };
    let card = h.query_all_by_label(&notes).next();
    card.ok_or("its notes on the card")?.click();
    h.run();
    assert_eq!(h.state().session().room(), Room::Notes);
    Ok(())
}
