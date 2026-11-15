//! The library's search: typing filters, suggests and shows where a book is.

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::library::Pick;
use mareader_engine::library::{Action, SearchAction, ShelfAction};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::library::search::LABEL;
use mareader_ui_egui::ui_command::Sent;

use crate::common::reader;

/// Ctrl+F, then the text, as typed into the bar.
fn search(h: &mut Harness<'static, MareaderApp>, text: &str) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run();
    h.get_by_label(LABEL).type_text(text);
    h.run();
}

fn focused(h: &Harness<'static, MareaderApp>) -> bool {
    h.get_by_label(LABEL).is_focused()
}

/// Cards and suggestion rows named `title`.
fn shown(h: &Harness<'static, MareaderApp>, title: &str) -> usize {
    h.query_all_by_label(title).count()
}

fn text(h: &Harness<'static, MareaderApp>) -> String {
    h.state().library_state().search_text().to_owned()
}

fn book_titled(h: &Harness<'static, MareaderApp>, title: &str) -> Pick {
    let books = h.state().library().books();
    let book = books.into_iter().find(|b| b.title.value == title);
    Pick::Book(book.expect("seeded").id)
}

#[test]
fn typing_filters_the_page_and_says_how_many_match() {
    let options = [("books", "10"), ("layout", "list"), ("library", "show")];
    let mut h = reader(&options);
    assert_eq!(shown(&h, "The Raven"), 1);
    search(&mut h, "essays");
    assert!(focused(&h));
    assert!(h.state().library_state().field().open, "suggestions show");
    // Enter goes to the best match; the filter stays.
    h.key_press(Key::Enter);
    h.run();
    assert!(!focused(&h));
    assert!(!h.state().library_state().field().open);
    assert_eq!(text(&h), "essays");
    assert_eq!(shown(&h, "Collected Essays, Volume 2"), 1);
    assert_eq!(shown(&h, "Collected Essays, Volume 10"), 1);
    assert_eq!(shown(&h, "The Raven"), 0);
    assert!(h.query_by_label("2 of 10 books").is_some());
}

#[test]
fn fuzzy_words_and_authors_match_in_any_order() {
    let options = [("books", "10"), ("layout", "list"), ("library", "show")];
    let mut h = reader(&options);
    search(&mut h, "woolf voyge");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(shown(&h, "The Voyage Out"), 1);
    assert!(h.query_by_label("1 of 10 books").is_some());
}

#[test]
fn arrows_and_enter_show_a_book_from_another_shelf() {
    let options = [
        ("books", "10"),
        ("shelves", "demo"),
        ("shelf", "To read"),
        ("library", "show"),
    ];
    let mut h = reader(&options);
    let empty = h.state().library_state().at();
    assert!(empty.is_some());
    search(&mut h, "rust");
    // Nothing on this shelf matches; the rest of the library does.
    assert!(h.query_by_label("No books match “rust”.").is_some());
    // The first suggestion is lit already.
    assert_eq!(h.state().library_state().field().lit, Some(0));
    h.key_press(Key::Enter);
    h.run_steps(2);
    let state = h.state().library_state();
    assert_eq!(state.at(), None, "it went where the book shows");
    assert_eq!(state.revealed(), Some(book_titled(&h, "Practical Rust")));
    assert_eq!(state.search_text(), "rust", "the filter stays");
    assert!(!focused(&h));
    assert_eq!(shown(&h, "Practical Rust"), 1);
    // Still between pulse and fade, it asks for no frames.
    h.run_steps(300);
    assert_eq!(h.state().library_state().revealed(), None, "the ring fades");
}

#[test]
fn choosing_a_shelf_opens_its_parent_with_it_lit() {
    let options = [
        ("books", "10"),
        ("shelves", "demo"),
        ("shelf", "Essays"),
        ("library", "show"),
    ];
    let mut h = reader(&options);
    search(&mut h, "clsc");
    h.key_press(Key::Enter);
    h.run_steps(2);
    let lib = h.state().library();
    let fiction = lib
        .shelves()
        .find(|s| s.name.value == "Fiction")
        .unwrap()
        .id;
    let classics = lib
        .shelves()
        .find(|s| s.name.value == "Classics")
        .unwrap()
        .id;
    let state = h.state().library_state();
    assert_eq!(state.at(), Some(fiction));
    assert_eq!(state.revealed(), Some(Pick::Shelf(classics)));
}

#[test]
fn a_suggestion_is_chosen_with_a_click() {
    let options = [("books", "10"), ("layout", "list"), ("library", "show")];
    let mut h = reader(&options);
    search(&mut h, "middle");
    // The page's card is below the row; the list sits on top.
    let rows = h.query_all_by_label("Middlemarch");
    let row = rows.min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
    row.unwrap().click();
    h.run_steps(2);
    let picked = book_titled(&h, "Middlemarch");
    assert_eq!(h.state().library_state().revealed(), Some(picked));
    assert!(!h.state().library_state().field().open);
}

#[test]
fn a_shelf_without_matches_offers_the_ones_elsewhere() {
    let options = [
        ("books", "10"),
        ("shelves", "demo"),
        ("shelf", "To read"),
        ("library", "show"),
    ];
    let mut h = reader(&options);
    search(&mut h, "raven");
    h.key_press(Key::Escape);
    h.run();
    h.get_by_label("Show the 1 match elsewhere").click();
    h.run();
    assert_eq!(h.state().library_state().at(), None);
    assert_eq!(shown(&h, "The Raven"), 1);
    h.get_by_label("Clear search").click();
    h.run();
    assert_eq!(text(&h), "");
    assert_eq!(shown(&h, "Middlemarch"), 1);
}

#[test]
fn escape_peels_the_list_the_text_the_focus_then_the_page() {
    let options = [("pdf", "sample:5"), ("books", "4"), ("library", "show")];
    let mut h = reader(&options);
    search(&mut h, "raven");
    assert!(
        !h.state().find().is_open(),
        "find searches the library here"
    );
    let escape = |h: &mut Harness<'static, MareaderApp>| {
        h.key_press(Key::Escape);
        h.run();
    };
    escape(&mut h);
    assert!(!h.state().library_state().field().open);
    assert_eq!(text(&h), "raven");
    escape(&mut h);
    assert_eq!(text(&h), "");
    assert!(focused(&h));
    escape(&mut h);
    assert!(!focused(&h));
    assert!(h.state().library_showing());
    // Unfocused, Escape clears a search before it leaves the page.
    search(&mut h, "raven");
    h.key_press(Key::Enter);
    h.run();
    escape(&mut h);
    assert_eq!(text(&h), "");
    assert!(h.state().library_showing());
    escape(&mut h);
    assert!(!h.state().library_showing());
}

#[test]
fn a_search_at_the_top_lists_nested_shelves_and_lights_them_there() {
    let options = [("books", "10"), ("shelves", "demo"), ("library", "show")];
    let mut h = reader(&options);
    search(&mut h, "clsc");
    h.key_press(Key::Enter);
    h.run_steps(2);
    let state = h.state().library_state();
    assert_eq!(state.at(), None, "it shows at the top already");
    assert!(matches!(state.revealed(), Some(Pick::Shelf(_))));
    assert_eq!(shown(&h, "Classics"), 1);
}

#[test]
fn up_from_the_first_suggestion_closes_and_down_reopens() {
    let options = [("books", "10"), ("layout", "list"), ("library", "show")];
    let mut h = reader(&options);
    search(&mut h, "essays");
    assert_eq!(h.state().library_state().field().lit, Some(0));
    h.key_press(Key::ArrowDown);
    h.run();
    assert_eq!(h.state().library_state().field().lit, Some(1));
    h.key_press(Key::ArrowDown);
    h.run();
    assert_eq!(h.state().library_state().field().lit, Some(1), "it stops");
    for _ in 0..2 {
        h.key_press(Key::ArrowUp);
        h.run();
    }
    assert!(!h.state().library_state().field().open);
    h.key_press(Key::ArrowDown);
    h.run();
    let field = h.state().library_state().field();
    assert!(field.open && field.lit == Some(0));
    // Nothing matches: no list at all.
    search(&mut h, "zzqx");
    assert!(h.query_by_label_contains("Enter goes there").is_none());
}

#[test]
fn going_into_a_shelf_the_search_found_ends_the_search() {
    let options = [("books", "10"), ("shelves", "demo"), ("library", "show")];
    let mut h = reader(&options);
    let act = |h: &mut Harness<'static, MareaderApp>, a| {
        h.state_mut().queue(Sent::Engine(Command::Library(a)));
        h.run();
    };
    act(&mut h, Action::Search(SearchAction::Text("fict".into())));
    let fiction = h.state().library().shelves();
    let fiction = fiction.into_iter().find(|s| s.name.value == "Fiction");
    let fiction = fiction.unwrap().id;
    act(&mut h, Action::Shelf(ShelfAction::GoTo(Some(fiction))));
    assert_eq!(text(&h), "");
    // Going back up keeps a search.
    act(&mut h, Action::Search(SearchAction::Text("raven".into())));
    act(&mut h, Action::Shelf(ShelfAction::GoTo(None)));
    assert_eq!(text(&h), "raven");
}
