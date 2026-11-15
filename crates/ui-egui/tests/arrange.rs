//! The arranged order: cards carried through a living grid and dropped.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::library::{Action, Named, ShelfAction};
use mareader_library::{Home, Item, Library, ShelfId, SortKey};
use mareader_ui_egui::MareaderApp;

use crate::common::{act, carry, release};

fn reader(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("library", "show").ok();
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn library<'a>(h: &'a Harness<'static, MareaderApp>) -> &'a Library {
    h.state().library()
}

fn top(h: &Harness<'static, MareaderApp>) -> Vec<Item> {
    library(h).arranged(None, Home::Everything)
}

/// What a card shows as its label.
fn label(h: &Harness<'static, MareaderApp>, item: Item) -> String {
    library(h).item_name(item).unwrap().to_owned()
}

/// A point `along` the card's width, from its left, mid-height.
fn on(h: &Harness<'static, MareaderApp>, item: Item, along: f32) -> egui::Pos2 {
    let rect = h.get_by_label(&label(h, item)).rect();
    egui::pos2(rect.left() + rect.width() * along, rect.center().y)
}

fn shelve(h: &mut Harness<'static, MareaderApp>, action: ShelfAction) {
    act(h, Action::Shelf(action));
}

/// Shelves are made through the page's own action, at the top level.
fn new_shelf(h: &mut Harness<'static, MareaderApp>, name: &str) -> Item {
    shelve(h, ShelfAction::New);
    let id = h.state().library_state().at().expect("a new shelf opens");
    act(h, Action::Rename(Named::Shelf(id), name.into()));
    shelve(h, ShelfAction::GoTo(None));
    Item::Shelf(id)
}

/// Carries `item` by its middle to `along` the width of `onto`.
fn drop_on(h: &mut Harness<'static, MareaderApp>, item: Item, onto: Item, along: f32) {
    let (from, to) = (on(h, item, 0.5), on(h, onto, along));
    drag(h, from, to);
}

fn drag(h: &mut Harness<'static, MareaderApp>, from: egui::Pos2, to: egui::Pos2) {
    carry(h, from, to);
    release(h, to);
}

/// Holds the pointer still for `seconds`, so a rest takes.
fn rest(h: &mut Harness<'static, MareaderApp>, seconds: f32) {
    h.run_steps((seconds * 60.0).ceil() as usize);
    h.run();
}

/// Moves a held carry to `to` and rests there.
fn rest_at(h: &mut Harness<'static, MareaderApp>, to: egui::Pos2, seconds: f32) {
    h.hover_at(to);
    h.run();
    rest(h, seconds);
}

/// A shelf named `name` holding `books`, at the top level.
fn shelf_of(h: &mut Harness<'static, MareaderApp>, name: &str, books: &[Item]) -> ShelfId {
    let shelf = new_shelf(h, name);
    for &book in books {
        drop_on(h, book, shelf, 0.5);
    }
    let Item::Shelf(id) = shelf else {
        unreachable!("new_shelf makes shelves")
    };
    id
}

fn book_id(item: Item) -> mareader_library::BookId {
    match item {
        Item::Book(id) => id,
        _ => unreachable!("a book"),
    }
}

fn inside(h: &Harness<'static, MareaderApp>, id: ShelfId) -> Vec<Item> {
    library(h).arranged(Some(id), Home::Everything)
}

fn rect_of(h: &Harness<'static, MareaderApp>, item: Item) -> egui::Rect {
    h.get_by_label(&label(h, item)).rect()
}

#[test]
fn a_card_dropped_on_anothers_side_moves_there() {
    let mut h = reader(&[("books", "4")]);
    let before = top(&h);
    let (first, last) = (before[0], before[3]);
    drop_on(&mut h, last, first, 0.2);
    assert_eq!(top(&h), [last, before[0], before[1], before[2]]);
    // On the right half, it goes after.
    drop_on(&mut h, last, before[1], 0.8);
    assert_eq!(top(&h), [before[0], before[1], last, before[2]]);
}

#[test]
fn a_shelf_moves_by_its_edges_and_takes_books_in_its_middle() {
    let mut h = reader(&[("books", "3")]);
    let shelf = new_shelf(&mut h, "Poetry");
    let books = top(&h);
    assert_eq!(books.last(), Some(&shelf), "a new shelf joins the end");
    drop_on(&mut h, shelf, books[0], 0.2);
    assert_eq!(top(&h).first(), Some(&shelf), "shelves mix among books");
    // A book on its outer quarter lands beside it, not on it.
    let book = books[2];
    drop_on(&mut h, book, shelf, 0.1);
    assert_eq!(top(&h).first(), Some(&book));
    assert!(library(&h).shelved().is_empty());
    drop_on(&mut h, book, shelf, 0.5);
    assert_eq!(library(&h).shelved().len(), 1, "the middle files it");
}

#[test]
fn a_drop_while_sorted_shows_the_order_it_made() {
    let mut h = reader(&[("books", "3"), ("sort", "title")]);
    let shown = |h: &Harness<'static, MareaderApp>| {
        let mut items = top(h);
        items.sort_by_key(|i| label(h, *i));
        items
    };
    let sorted = shown(&h);
    drop_on(&mut h, sorted[2], sorted[0], 0.2);
    let view = h.state().settings().library;
    assert_eq!((view.sort, view.ascending), (SortKey::Arranged, true));
    let order = top(&h);
    let at = |item| order.iter().position(|i| *i == item);
    assert_eq!(at(sorted[2]).map(|n| n + 1), at(sorted[0]), "{order:?}");
}

#[test]
fn the_list_files_by_drag_but_keeps_its_order() {
    let mut h = reader(&[("books", "3"), ("layout", "list")]);
    let before = top(&h);
    let (first, last) = (before[0], before[2]);
    let row = |h: &Harness<'static, MareaderApp>, item| {
        let rect = h.get_by_label(&label(h, item)).rect();
        egui::pos2(rect.center().x, rect.top() + 4.0)
    };
    let (from, to) = (row(&h, last), row(&h, first));
    drag(&mut h, from, to);
    assert_eq!(top(&h), before);
}

#[test]
fn carried_cards_make_room_and_settle_where_dropped() {
    let mut h = reader(&[("books", "4")]);
    let order = top(&h);
    let (first, last) = (order[0], order[3]);
    let (slot0, slot1) = (rect_of(&h, first), rect_of(&h, order[1]));
    let (from, to) = (on(&h, last, 0.5), on(&h, first, 0.2));
    carry(&mut h, from, to);
    // Passing over cards moves nothing; resting on one makes room.
    assert!((rect_of(&h, first).left() - slot0.left()).abs() < 1.0);
    h.run_steps(20);
    h.run();
    // Lifted, it leaves the grid; the first card slides into the next slot.
    assert!(h.query_by_label(&label(&h, last)).is_none(), "lifted");
    assert!((rect_of(&h, first).left() - slot1.left()).abs() < 1.0);
    release(&mut h, to);
    assert_eq!(top(&h), [last, order[0], order[1], order[2]]);
    assert!(
        (rect_of(&h, last).min - slot0.min).length() < 1.0,
        "settled"
    );
}

#[test]
fn escape_puts_a_lifted_card_back() {
    let mut h = reader(&[("books", "4")]);
    let order = top(&h);
    let (first, last) = (order[0], order[3]);
    let (home, was) = (rect_of(&h, first), rect_of(&h, last));
    let (from, to) = (on(&h, last, 0.5), on(&h, first, 0.2));
    carry(&mut h, from, to);
    h.key_press(egui::Key::Escape);
    h.run();
    release(&mut h, to);
    assert_eq!(top(&h), order);
    assert!((rect_of(&h, first).min - home.min).length() < 1.0);
    assert!((rect_of(&h, last).min - was.min).length() < 1.0);
}

#[test]
fn resting_a_book_on_another_makes_a_shelf_in_its_place() {
    let mut h = reader(&[("books", "4")]);
    let order = top(&h);
    let (onto, carried) = (order[1], order[3]);
    let (from, to) = (on(&h, carried, 0.5), on(&h, onto, 0.5));
    carry(&mut h, from, to);
    rest(&mut h, 0.4);
    release(&mut h, to);
    let now = top(&h);
    let Some(&Item::Shelf(id)) = now.get(1) else {
        panic!("no shelf where the book was: {now:?}");
    };
    let mut on_it = inside(&h, id);
    on_it.sort();
    let mut both = vec![onto, carried];
    both.sort();
    assert_eq!(on_it, both);
    // Every book still shows at the top, the shelf before its first.
    assert_eq!(now.len(), 5);
    assert_eq!(now.get(2), Some(&onto));
}

#[test]
fn a_quick_drop_on_a_books_middle_only_moves_it() {
    let mut h = reader(&[("books", "4")]);
    let order = top(&h);
    drop_on(&mut h, order[3], order[1], 0.6);
    assert_eq!(top(&h), [order[0], order[1], order[3], order[2]]);
    assert_eq!(library(&h).shelves().count(), 0);
}

#[test]
fn resting_on_a_shelf_opens_it_to_drop_inside_in_place() {
    let mut h = reader(&[("books", "4")]);
    let order = top(&h);
    let id = shelf_of(&mut h, "Poetry", &[order[0], order[1]]);
    let was = inside(&h, id);
    assert_eq!(was.len(), 2);
    let book = order[3];
    let (from, to) = (on(&h, book, 0.5), on(&h, Item::Shelf(id), 0.5));
    carry(&mut h, from, to);
    rest(&mut h, 0.8);
    assert_eq!(h.state().library_state().at(), Some(id), "opened");
    // Inside, resting on the first book's edge makes room before it.
    let first = on(&h, was[0], 0.1);
    rest_at(&mut h, first, 0.3);
    release(&mut h, first);
    assert_eq!(inside(&h, id), [book, was[0], was[1]]);
}

#[test]
fn resting_on_a_crumb_goes_up_to_drop_there_in_place() {
    let mut h = reader(&[("books", "3")]);
    let order = top(&h);
    let id = shelf_of(&mut h, "Poetry", &[order[0], order[1]]);
    shelve(&mut h, ShelfAction::GoTo(Some(id)));
    let book = order[1];
    let crumb = h
        .get_by_role_and_label(Role::Button, "Home")
        .rect()
        .center();
    let from = on(&h, book, 0.5);
    carry(&mut h, from, crumb);
    rest(&mut h, 0.8);
    assert_eq!(h.state().library_state().at(), None, "went up");
    let first = on(&h, order[0], 0.1);
    rest_at(&mut h, first, 0.3);
    release(&mut h, first);
    assert!(
        library(&h).shelves_of(book_id(book)).is_empty(),
        "off the shelf"
    );
    assert_eq!(top(&h)[..2], [book, order[0]]);
}

/// Presses on `at` and holds still for `seconds`.
fn press_hold(h: &mut Harness<'static, MareaderApp>, at: egui::Pos2, seconds: f32) {
    h.hover_at(at);
    h.run();
    h.drag_at(at);
    h.run();
    rest(h, seconds);
}

#[test]
fn a_long_press_picks_a_card_and_its_release_is_no_click() {
    let mut h = reader(&[("books", "3")]);
    let order = top(&h);
    let at = on(&h, order[1], 0.5);
    press_hold(&mut h, at, 0.6);
    release(&mut h, at);
    let picks = h.state().library_state().picks().clone();
    assert!(picks.contains(order[1]) && picks.len() == 1, "{picks:?}");
    // Picking now, a plain click adds another.
    h.get_by_label(&label(&h, order[2])).click();
    h.run();
    assert_eq!(h.state().library_state().picks().len(), 2);
}

#[test]
fn holding_on_lifts_the_whole_selection_to_drop_together() {
    let mut h = reader(&[("books", "4")]);
    let order = top(&h);
    let id = shelf_of(&mut h, "Poetry", &[]);
    let a = on(&h, order[0], 0.5);
    press_hold(&mut h, a, 0.6);
    release(&mut h, a);
    h.get_by_label(&label(&h, order[2])).click();
    h.run();
    // Held past a click's length, the press lifts both.
    press_hold(&mut h, a, 1.0);
    let lifted =
        |h: &Harness<'static, MareaderApp>, item| h.query_by_label(&label(h, item)).is_none();
    assert!(lifted(&h, order[0]) && lifted(&h, order[2]), "both lifted");
    assert!(!lifted(&h, order[1]));
    let to = on(&h, Item::Shelf(id), 0.5);
    rest_at(&mut h, to, 0.1);
    release(&mut h, to);
    let mut filed = inside(&h, id);
    filed.sort();
    let mut both = vec![order[0], order[2]];
    both.sort();
    assert_eq!(filed, both);
}

#[test]
fn a_selection_rested_on_a_book_makes_one_shelf_of_all() {
    let mut h = reader(&[("books", "4")]);
    let order = top(&h);
    let a = on(&h, order[0], 0.5);
    press_hold(&mut h, a, 0.6);
    release(&mut h, a);
    h.get_by_label(&label(&h, order[1])).click();
    h.run();
    let to = on(&h, order[3], 0.5);
    carry(&mut h, a, to);
    rest(&mut h, 0.4);
    release(&mut h, to);
    let shelf = top(&h).into_iter().find(|i| matches!(i, Item::Shelf(_)));
    let Some(Item::Shelf(id)) = shelf else {
        panic!("no shelf made");
    };
    assert_eq!(inside(&h, id).len(), 3);
    assert!(h.state().library_state().picks().is_empty(), "picks let go");
}
