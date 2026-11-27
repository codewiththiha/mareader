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
    assert!(h.query_by_label("AI selection results 0").is_some());
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

#[test]
fn note_context_selection_and_two_step_delete() {
    let mut h = marked("notes");
    let before = h.state().session().library_model().every_mark().count();
    h.get_by_label("Open The mind is its own place.")
        .click_secondary();
    h.run_steps(4);
    h.get_by_label("Select").click();
    h.run_steps(4);
    h.get_by_label("Delete selected…").click();
    h.run_steps(4);
    h.get_by_label("Delete").click();
    h.run_steps(4);
    assert_eq!(
        h.state().session().library_model().every_mark().count(),
        before
    );
    h.get_by_label("Are you sure?").click();
    h.run_steps(6);
    assert_eq!(
        h.state().session().library_model().every_mark().count(),
        before - 1
    );
}

#[test]
fn ai_selection_answers_can_be_selected_and_deleted() {
    let mut h = reader(&[("books", "3"), ("ai_notes", "demo"), ("space", "notes")]);
    h.set_size(egui::vec2(1100.0, 900.0));
    h.run_steps(10);
    assert!(h.query_by_label("AI selection results 1").is_some());
    h.get_by_label("AI selection results 1").click();
    h.run_steps(4);
    let answer = h.get_by_label("A selected passage");
    answer.click_secondary();
    h.run_steps(4);
    h.get_by_label("Select").click();
    h.run_steps(4);
    h.get_by_label("Delete selected…").click();
    h.run_steps(4);
    h.get_by_label("Delete").click();
    h.run_steps(4);
    h.get_by_label("Are you sure?").click();
    h.run_steps(6);
    assert!(h.query_by_label("AI selection results 0").is_some());
    assert!(h.query_by_label("A selected passage").is_none());
}

#[test]
fn note_navigation_returns_to_filtered_notes() -> Result<(), Box<dyn std::error::Error>> {
    use mareader_engine::{command::Command, library::Action, route::Route};
    use mareader_ui_egui::ui_command::Sent;
    let raven = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../epub/tests/books/the-raven-gutenberg-17192.epub")
        .canonicalize()?;
    let mut h = reader(&[("epub", raven.to_str().ok_or("UTF-8 path")?)]);
    h.run_steps(10);
    h.state_mut()
        .queue(Sent::Engine(Command::Library(Action::SeedMarks)));
    h.state_mut()
        .queue(Sent::Engine(Command::ShowSpace(Room::Notes)));
    h.run_steps(10);
    h.get_by_label("Notes 4").click();
    h.run_steps(5);
    h.get_by_label("Open Nothing that we despise in other men is inherently absurd.")
        .click();
    h.run_steps(30);
    assert_eq!(h.state().route(), Route::Reader);
    h.get_by_label("Go back to notes").click();
    h.run_steps(10);
    assert_eq!(h.state().session().room(), Room::Notes);
    assert!(
        h.query_by_label("Open Nothing that we despise in other men is inherently absurd.")
            .is_some()
    );
    assert!(
        h.query_by_label("Open The mind is its own place.")
            .is_none()
    );
    Ok(())
}

#[test]
fn ai_tab_filters_marks_and_all_keeps_both_kinds() {
    let mut h = reader(&[
        ("books", "3"),
        ("marks", "demo"),
        ("ai_notes", "demo"),
        ("space", "notes"),
    ]);
    h.set_size(egui::vec2(1280.0, 900.0));
    h.run_steps(10);
    assert!(h.query_by_label("AI selection results 1").is_some());
    assert!(h.query_by_label("A selected passage").is_some());
    assert!(
        h.query_by_label("Open The mind is its own place.")
            .is_some()
    );
    h.get_by_label("AI selection results 1").click();
    h.run_steps(5);
    assert!(h.query_by_label("A selected passage").is_some());
    assert!(
        h.query_by_label("Open The mind is its own place.")
            .is_none()
    );
    h.get_by_label("All 10").click();
    h.run_steps(5);
    assert!(
        h.query_by_label("Open The mind is its own place.")
            .is_some()
    );
    assert!(h.query_by_label("A selected passage").is_some());
}

#[test]
fn both_room_sidebars_return_to_space_at_the_bottom() {
    for room in ["notes", "words"] {
        let mut h = marked(room);
        let button = h.get_by_label("Back to Space");
        let rect = button.rect();
        assert!(
            rect.bottom() > 800.0,
            "{room} back button stays at the foot: {rect:?}"
        );
        button.click();
        h.run_steps(5);
        assert_eq!(h.state().session().room(), Room::Hub);
    }
}

#[test]
fn narrow_room_back_control_still_reaches_the_hub() {
    for room in ["notes", "words"] {
        let mut h = marked(room);
        h.set_size(egui::vec2(400.0, 650.0));
        h.run_steps(5);
        h.get_by_label("Back to Space").click();
        h.run_steps(5);
        assert_eq!(h.state().session().room(), Room::Hub);
    }
}

#[test]
fn narrow_notes_expose_all_four_tabs() {
    let mut h = reader(&[("books", "3"), ("ai_notes", "demo"), ("space", "notes")]);
    h.set_size(egui::vec2(400.0, 650.0));
    h.run_steps(8);
    let tab = h.get_by_label("AI selection results 1");
    assert!(tab.rect().right() < 400.0, "the fourth tab is not clipped");
    tab.click();
    h.run_steps(5);
    assert!(h.query_by_label("A selected passage").is_some());
}

#[test]
fn highlight_and_bookmark_clicks_offer_the_reader_return() -> Result<(), Box<dyn std::error::Error>> {
    use mareader_engine::{command::Command, library::Action, route::Route};
    use mareader_ui_egui::ui_command::Sent;
    let raven = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../epub/tests/books/the-raven-gutenberg-17192.epub").canonicalize()?;
    for (tab, label) in [("All 9", "Open The mind is its own place."), ("Bookmarks 2", "Open Chapter 3")] {
        let mut h = reader(&[("epub", raven.to_str().ok_or("UTF-8 path")?)]);
        h.set_size(egui::vec2(1280.0, 900.0));
        h.run_steps(10);
        h.state_mut().queue(Sent::Engine(Command::Library(Action::SeedMarks)));
        h.state_mut().queue(Sent::Engine(Command::ShowSpace(Room::Notes)));
        h.run_steps(10);
        h.get_by_label(tab).click();
        h.run_steps(4);
        h.get_by_label(label).click();
        h.run_steps(30);
        assert_eq!(h.state().route(), Route::Reader);
        h.get_by_label("Go back to notes").click();
        h.run_steps(8);
        assert_eq!(h.state().session().room(), Room::Notes);
        assert!(h.query_by_label(label).is_some(), "returned to {label}");
    }
    Ok(())
}

#[test]
fn saved_ai_selection_opens_and_returns_to_its_tab() -> Result<(), Box<dyn std::error::Error>> {
    use mareader_engine::{command::Command, library::Action, route::Route, skills::Anchor};
    use mareader_ui_egui::{glow, ui_command::Sent};
    let raven = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../epub/tests/books/the-raven-gutenberg-17192.epub").canonicalize()?;
    let mut h = reader(&[("epub", raven.to_str().ok_or("UTF-8 path")?)]);
    h.set_size(egui::vec2(1280.0, 900.0));
    h.run_steps(30);
    let pane = h.state().active_pane();
    let document = h.state().session().documents().document_of(pane).ok_or("document")?;
    let (page, text) = (0..document.page_count()).find_map(|page| {
        let layer = document.text_layer(page).ok()?;
        let start = (0..layer.text.len().saturating_sub(25))
            .find(|&i| (i..i + 25).all(|n| layer.place(n).is_some()))?;
        let text = layer.text.text(start..start + 25);
        (!text.trim().is_empty()).then_some((page, text))
    }).ok_or("selectable EPUB text")?;
    h.state_mut().queue(Sent::Engine(Command::SelectText { pane, page, text }));
    h.run_steps(5);
    let source = h.state().session().documents().highlighted(pane).ok_or("selection")?.1;
    h.state_mut().queue(Sent::Engine(Command::Library(Action::SeedAiNotes)));
    h.state_mut().queue(Sent::Engine(Command::ShowSpace(Room::Notes)));
    h.run_steps(15);
    h.get_by_label("AI selection results 1").click();
    h.run_steps(5);
    h.get_by_label(&source).click();
    h.run_steps(80);
    assert_eq!(h.state().route(), Route::Reader);
    let ask = h.state().skills().asks().iter().find(|a| a.pinned.is_some()).ok_or("restored answer")?;
    assert!(ask.open, "the saved answer is opened");
    assert!(matches!(&ask.anchor, Anchor::Text(spans) if !spans.is_empty()));
    assert!(glow::shown(&h.ctx) > 0, "the reader shows the AI selection outline");
    h.get_by_label("Go back to notes").click();
    h.run_steps(8);
    assert_eq!(h.state().session().room(), Room::Notes);
    assert!(h.query_by_label(&source).is_some(), "the AI tab is restored");
    Ok(())
}
