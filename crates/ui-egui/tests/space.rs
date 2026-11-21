//! Space, headless: the library bar's way in, the tabs and their counts, and
//! a word memorized or forgotten from its row.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::route::Route;
use mareader_library::WordStatus;
use mareader_ui_egui::MareaderApp;

use crate::common::{press, reader};

type H = Harness<'static, MareaderApp>;

/// Three books and the made-up words studied in them, on `page`.
fn shelf(page: &str) -> H {
    reader(&[("books", "3"), ("words", "demo"), ("route", page)])
}

fn status(h: &H, lemma: &str) -> Option<WordStatus> {
    let lib = h.state().session().library_model();
    let found = lib.words().find(|w| w.lemma == lemma);
    found.map(|w| w.status.value)
}

#[test]
fn the_library_bar_opens_space_on_every_sighting() {
    let mut h = shelf("library");
    assert_eq!(h.state().route(), Route::Library);
    h.get_by_label("Your words").click();
    h.run();
    assert_eq!(h.state().route(), Route::Space);
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
    let mut h = shelf("space");
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
    let mut h = shelf("space");
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
fn escape_goes_back_to_the_library() {
    let mut h = shelf("space");
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().route(), Route::Library);
}
