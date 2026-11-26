//! The empty-space bulk missing-books action.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::library::{Action, BookAction, Scope, SearchAction, Sheet, ShelfAction};
use mareader_library::{BookId, Move};
use mareader_ui_egui::MareaderApp;

use crate::common::{act, reader};

const LABEL: &str = "Remove all missing books…";

fn menu(h: &mut Harness<'static, MareaderApp>) {
    let pos = egui::pos2(450.0, 620.0);
    h.hover_at(pos);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(3);
}

fn book(h: &mut Harness<'static, MareaderApp>, action: BookAction) {
    act(h, Action::Book(action));
}

fn missing(h: &Harness<'static, MareaderApp>) -> BookId {
    h.state()
        .library()
        .books()
        .find(|b| b.missing())
        .unwrap()
        .id
}

fn twin(h: &mut Harness<'static, MareaderApp>, id: BookId) -> BookId {
    let before: Vec<_> = h.state().library().books().map(|b| b.id).collect();
    book(h, BookAction::Twin(id));
    h.state()
        .library()
        .books()
        .find(|b| !before.contains(&b.id))
        .unwrap()
        .id
}

#[test]
fn threshold_and_cancel() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let first = missing(&h);
    menu(&mut h);
    assert!(h.query_by_label(LABEL).is_none());
    h.key_press(egui::Key::Escape);
    h.run();
    let second = twin(&mut h, first);
    menu(&mut h);
    h.get_by_label(LABEL).click();
    h.run();
    assert!(
        matches!(h.state().library_state().sheet(), Some(Sheet::MissingRemoval { books }) if books.len() == 2)
    );
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.state().library().book(first).is_some());
    assert!(h.state().library().book(second).is_some());
}

#[test]
fn shelf_scope_and_confirmation_remove_only_its_missing_books() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let first = missing(&h);
    let second = twin(&mut h, first);
    let third = twin(&mut h, first);
    let shelf = h.state_mut().seed_library_shelf("My shelf").unwrap();
    let mv = Move {
        books: vec![first, second],
        shelves: vec![],
        links: vec![],
        from: None,
        to: Some(shelf),
        keep: false,
        before: None,
    };
    act(&mut h, Action::Shelf(ShelfAction::Carry(mv)));
    act(&mut h, Action::Shelf(ShelfAction::GoTo(Some(shelf))));
    menu(&mut h);
    h.get_by_label(LABEL).click();
    h.run();
    assert!(
        matches!(h.state().library_state().sheet(), Some(Sheet::MissingRemoval { books }) if books.len() == 2 && !books.contains(&third))
    );
    h.get_by_label("Remove").click();
    h.run();
    assert!(h.state().library().book(first).is_none());
    assert!(h.state().library().book(second).is_none());
    assert!(h.state().library().book(third).is_some());
    assert!(h.state().library().shelf(shelf).is_some());
    assert!(h.state().library().books().any(|b| !b.missing()));
}

#[test]
fn search_does_not_offer_ambiguous_bulk_action() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let first = missing(&h);
    twin(&mut h, first);
    act(&mut h, Action::Search(SearchAction::Text("book".into())));
    menu(&mut h);
    assert!(h.query_by_label(LABEL).is_none());
}

#[test]
fn library_home_does_not_remove_missing_books_on_other_shelves() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let first = missing(&h);
    let second = twin(&mut h, first);
    let third = twin(&mut h, first);
    let shelf = h.state_mut().seed_library_shelf("Other shelf").unwrap();
    let mv = Move {
        books: vec![third],
        shelves: vec![],
        links: vec![],
        from: None,
        to: Some(shelf),
        keep: false,
        before: None,
    };
    act(&mut h, Action::Shelf(ShelfAction::Carry(mv)));
    act(&mut h, Action::Scope(Scope::Missing));
    menu(&mut h);
    h.get_by_label(LABEL).click();
    h.run();
    assert!(
        matches!(h.state().library_state().sheet(), Some(Sheet::MissingRemoval { books })
        if books.contains(&first) && books.contains(&second) && !books.contains(&third))
    );
    h.get_by_label("Remove").click();
    h.run();
    assert!(h.state().library().book(first).is_none());
    assert!(h.state().library().book(second).is_none());
    assert!(h.state().library().book(third).is_some());
}
