//! Space's notes, headless: the hub's card in, the chips and the books
//! narrowing the rows, and a row asking for its book at the mark.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::space::Room;
use mareader_ui_egui::MareaderApp;

use crate::common::reader;

type H = Harness<'static, MareaderApp>;

/// Three books, marked, on Space's `room`, in a window wide enough for the
/// rail.
fn marked(room: &str) -> H {
    let mut h = reader(&[("books", "3"), ("marks", "demo"), ("space", room)]);
    h.set_size(egui::vec2(1280.0, 900.0));
    h.run();
    h
}

#[test]
fn the_notes_card_opens_every_highlight_note_and_bookmark() {
    let mut h = marked("hub");
    h.get_by_label("Notes").click();
    h.run();
    assert_eq!(h.state().session().room(), Room::Notes);
    assert!(h.query_by_label("All 9").is_some());
    assert!(h.query_by_label("Notes 4").is_some());
    assert!(h.query_by_label("Bookmarks 2").is_some());
    assert!(
        h.query_by_label("Open The mind is its own place.")
            .is_some()
    );
}

#[test]
fn a_chip_and_a_book_narrow_the_rows() {
    let mut h = marked("notes");
    h.get_by_label("Bookmarks 2").click();
    h.run();
    assert!(h.query_by_label("Open Chapter 3").is_some());
    assert!(
        h.query_by_label("Open The mind is its own place.")
            .is_none()
    );
    h.get_by_label("All 9").click();
    h.run();
    let (title, held) = {
        let lib = h.state().session().library_model();
        let first = lib
            .books()
            .next()
            .map(|b| (b.title.value.clone(), lib.marks(b.id).len()));
        first.expect("a book")
    };
    assert!(held > 0 && held < 9, "{title} holds {held}");
    // The rail's row comes before the sheet's heading of the same name.
    let row = h.query_all_by_label(&title).next();
    row.expect("the book's row in the rail").click();
    h.run();
    let all = format!("All {held}");
    assert!(h.query_by_label(&all).is_some(), "{title}'s own {held}");
    h.get_by_label("Back to Space").click();
    h.run();
    assert_eq!(h.state().session().room(), Room::Hub);
}
