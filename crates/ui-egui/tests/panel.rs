//! The rail's Library panel: a click opens as set, a drag places a book.

use std::path::{Path, PathBuf};

use egui::Key;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use mareader_engine::command::Command;
use mareader_engine::library::{Action, BookAction};
use mareader_library::{BookId, Library};
use mareader_settings::LibraryClick;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::sidebar::WIDTH;
use mareader_ui_egui::ui_command::Sent;
use mareader_workspace::PaneId;

use crate::common::reader;

fn raven() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../epub/tests/books");
    dir.join("the-raven-gutenberg-17192.epub")
        .canonicalize()
        .unwrap()
}

fn library<'a>(h: &'a Harness<'static, MareaderApp>) -> &'a Library {
    h.state().library()
}

/// The Raven open, its twin in the library, the rail on Library.
fn raven_and_twin(click: &str) -> (Harness<'static, MareaderApp>, BookId, BookId) {
    let path = raven();
    let options = [
        ("epub", path.to_str().unwrap()),
        ("library_click", click),
        ("float", "false"),
        ("sidebar", "library"),
    ];
    let mut h = reader(&options);
    h.run_steps(3);
    let id = library(&h).book_at(&path).expect("recorded");
    let twin = Sent::Engine(Command::Library(Action::Book(BookAction::Twin(id))));
    h.state_mut().queue(twin);
    h.run();
    h.run();
    let twin = library(&h).books().find(|b| b.twin_of.value == Some(id));
    let twin = twin.expect("twinned").id;
    (h, id, twin)
}

/// The tree's row for `title`: in the rail, below any pane tabs.
fn row(h: &Harness<'static, MareaderApp>, title: &str) -> egui::Rect {
    let rows = h.query_all_by_label(title).map(|n| n.rect());
    let in_rail = rows.filter(|r| r.left() < WIDTH);
    let lowest = in_rail.max_by(|a, b| a.top().total_cmp(&b.top()));
    lowest.unwrap_or_else(|| panic!("no row {title}"))
}

fn click_row(h: &mut Harness<'static, MareaderApp>, title: &str) {
    let at = row(h, title).center();
    tap(h, at);
}

fn tap(h: &mut Harness<'static, MareaderApp>, at: egui::Pos2) {
    h.hover_at(at);
    h.run();
    h.drag_at(at);
    h.run();
    release(h, at);
}

fn release(h: &mut Harness<'static, MareaderApp>, at: egui::Pos2) {
    h.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    h.run_steps(3);
}

/// Presses on `from` and carries the pointer to `to`, still held.
fn carry(h: &mut Harness<'static, MareaderApp>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    h.run();
    h.drag_at(from);
    h.run();
    for step in 1..=8 {
        h.hover_at(from.lerp(to, step as f32 / 8.0));
        h.run();
    }
}

fn reads(h: &Harness<'static, MareaderApp>, pane: PaneId) -> Option<BookId> {
    let app = h.state();
    app.documents().book(pane).or_else(|| {
        let place = app.documents().place(pane)?;
        app.library().book_at(&place.path)
    })
}

#[test]
fn shelves_open_in_place_and_list_their_shelves_and_books() {
    let options = [
        ("books", "10"),
        ("shelves", "demo"),
        ("pdf", "sample:3"),
        ("float", "false"),
        ("sidebar", "library"),
    ];
    let mut h = reader(&options);
    assert!(h.query_by_label("Classics").is_none(), "closed at first");
    let fiction = h.get_by_label("Fiction").rect().center();
    tap(&mut h, fiction);
    assert!(h.query_by_label("Classics").is_some(), "Fiction opened");
    // The keys close it again.
    h.get_by_label("Fiction").focus();
    h.run();
    h.key_press(Key::ArrowLeft);
    h.run();
    assert!(h.query_by_label("Classics").is_none());
}

#[test]
fn a_click_replaces_the_focused_pane_by_default() {
    let (mut h, _, twin) = raven_and_twin("replace");
    click_row(&mut h, "The Raven (2)");
    assert_eq!(h.state().panes().len(), 1);
    assert_eq!(reads(&h, h.state().active_pane()), Some(twin));
}

#[test]
fn a_click_can_open_beside_and_then_focuses_where_it_shows() {
    let (mut h, id, twin) = raven_and_twin("split");
    let first = h.state().active_pane();
    click_row(&mut h, "The Raven (2)");
    let beside = h.state().active_pane();
    assert_eq!(h.state().panes().len(), 2);
    assert_ne!(beside, first);
    assert_eq!(reads(&h, beside), Some(twin));
    assert_eq!(reads(&h, first), Some(id), "still the original");
    // Shown already, the original's row focuses its pane.
    click_row(&mut h, "The Raven");
    assert_eq!(h.state().panes().len(), 2);
    assert_eq!(h.state().active_pane(), first);
}

#[test]
fn do_nothing_leaves_clicks_to_drags_and_keys() {
    let (mut h, _, twin) = raven_and_twin("drag_only");
    click_row(&mut h, "The Raven (2)");
    assert_eq!(h.state().panes().len(), 1);
    assert_ne!(reads(&h, h.state().active_pane()), Some(twin));
    h.get_all_by_label("The Raven (2)").last().unwrap().focus();
    h.run();
    h.key_press(Key::Enter);
    h.run();
    h.run_steps(3);
    assert_eq!(h.state().panes().len(), 2, "Enter opens beside");
}

#[test]
fn a_book_dropped_on_a_pane_edge_splits_there() {
    let (mut h, id, twin) = raven_and_twin("drag_only");
    let first = h.state().active_pane();
    let from = row(&h, "The Raven (2)").center();
    let to = egui::pos2(880.0, 350.0);
    carry(&mut h, from, to);
    assert!(
        h.query_by_label_contains("Split right of").is_none(),
        "the preview is painted, not a widget"
    );
    release(&mut h, to);
    let panes = h.state().panes();
    assert_eq!(panes.len(), 2);
    let new = *panes.iter().find(|&&p| p != first).unwrap();
    assert_eq!(reads(&h, new), Some(twin));
    assert_eq!(reads(&h, first), Some(id));
    let rects = h.state().pane_rects();
    let x = |p| rects.iter().find(|(id, _)| *id == p).unwrap().1.left();
    assert!(x(new) > x(first), "on the right");
}

#[test]
fn a_book_dropped_on_an_empty_pane_opens_in_it() {
    let (mut h, _, twin) = raven_and_twin("replace");
    let first = h.state().active_pane();
    let split = Sent::Engine(Command::Split(mareader_workspace::SplitAxis::Horizontal));
    h.state_mut().queue(split);
    h.run();
    let empty = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::FocusPane(first)));
    h.run();
    let from = row(&h, "The Raven (2)").center();
    let to = egui::pos2(700.0, 120.0);
    carry(&mut h, from, to);
    release(&mut h, to);
    assert_eq!(h.state().panes().len(), 2, "no new split");
    assert_eq!(reads(&h, empty), Some(twin));
    assert_eq!(h.state().active_pane(), empty);
}

#[test]
fn while_split_the_panes_list_as_tabs_that_focus_and_close() {
    let (mut h, _, _) = raven_and_twin("split");
    let first = h.state().active_pane();
    click_row(&mut h, "The Raven (2)");
    assert!(h.query_by_label("Close The Raven (2)").is_some());
    let rows = h.query_all_by_label("The Raven").map(|n| n.rect());
    let in_rail = rows.filter(|r| r.left() < WIDTH);
    let tab = in_rail.min_by(|a, b| a.top().total_cmp(&b.top())).unwrap();
    tap(&mut h, tab.center());
    assert_eq!(h.state().active_pane(), first);
    h.get_by_label("Close The Raven (2)").click();
    h.run();
    assert_eq!(h.state().panes(), vec![first]);
}

#[test]
fn the_library_page_opens_beside_when_set_to_split() {
    let (mut h, _, twin) = raven_and_twin("split");
    h.state_mut().queue(Sent::Engine(Command::OpenBook(twin)));
    h.run();
    h.run_steps(3);
    assert_eq!(h.state().panes().len(), 2);
    assert_eq!(reads(&h, h.state().active_pane()), Some(twin));
    // Do nothing has no meaning on the page: it opens in place.
    let (mut h, _, twin) = raven_and_twin("drag_only");
    h.state_mut().queue(Sent::Engine(Command::OpenBook(twin)));
    h.run();
    h.run_steps(3);
    assert_eq!(h.state().panes().len(), 1);
    assert_eq!(reads(&h, h.state().active_pane()), Some(twin));
}

#[test]
fn the_workspace_tab_picks_what_a_click_does() {
    let mut h = reader(&[("settings", "panes")]);
    let click = |h: &Harness<'static, MareaderApp>| h.state().settings().workspace.library_click;
    assert_eq!(click(&h), LibraryClick::Replace);
    h.get_by_label("Open as a new split").click();
    h.run();
    assert_eq!(click(&h), LibraryClick::Split);
    h.get_by_label("Do nothing").click();
    h.run();
    assert_eq!(click(&h), LibraryClick::DragOnly);
}

#[test]
fn a_missing_book_is_shown_but_never_opened() {
    let options = [
        ("books", "4"),
        ("pdf", "sample:3"),
        ("float", "false"),
        ("sidebar", "library"),
    ];
    let mut h = reader(&options);
    let missing = library(&h).books().find(|b| b.missing()).unwrap();
    let (id, title) = (missing.id, missing.title.value.clone());
    let row = h.get_by_label(&title);
    assert!(row.accesskit_node().is_disabled(), "marked unavailable");
    let at = row.rect().center();
    let before = h.state().panes().len();
    tap(&mut h, at);
    h.run();
    assert_eq!(h.state().panes().len(), before);
    assert!(
        h.state().library_state().sheet().is_none(),
        "no find sheet here"
    );
    assert!(library(&h).book(id).unwrap().missing());
}

fn shelf_named(h: &Harness<'static, MareaderApp>, name: &str) -> mareader_library::ShelfId {
    let shelf = library(h).shelves().find(|s| s.name.value == name);
    shelf.unwrap_or_else(|| panic!("no shelf {name}")).id
}

fn demo() -> Harness<'static, MareaderApp> {
    let options = [
        ("books", "10"),
        ("shelves", "demo"),
        ("pdf", "sample:3"),
        ("float", "false"),
        ("sidebar", "library"),
    ];
    reader(&options)
}

#[test]
fn a_book_dragged_onto_a_shelf_row_moves_onto_it() {
    let mut h = demo();
    let to_read = shelf_named(&h, "To read");
    let lib = library(&h);
    let loose = lib
        .books()
        .find(|b| !b.missing() && lib.shelves_of(b.id).is_empty());
    let (book, title) = loose.map(|b| (b.id, b.title.value.clone())).unwrap();
    let from = row(&h, &title).center();
    let to = row(&h, "To read").center();
    carry(&mut h, from, to);
    release(&mut h, to);
    assert!(library(&h).shelves_of(book).contains(&to_read));
    assert_eq!(h.state().panes().len(), 1, "no pane opened");
}

#[test]
fn a_shelf_dragged_into_its_own_shelf_stays_put() {
    let mut h = demo();
    let (fiction, classics) = (shelf_named(&h, "Fiction"), shelf_named(&h, "Classics"));
    click_row(&mut h, "Fiction");
    let from = row(&h, "Fiction").center();
    let to = row(&h, "Classics").center();
    carry(&mut h, from, to);
    release(&mut h, to);
    let lib = library(&h);
    let parent = |id| lib.shelf(id).and_then(|s| lib.parent_of(s));
    assert_eq!(parent(fiction), None);
    assert_eq!(parent(classics), Some(fiction));
    // Onto another shelf, it nests there.
    let to = row(&h, "To read").center();
    let from = row(&h, "Fiction").center();
    carry(&mut h, from, to);
    release(&mut h, to);
    let lib = library(&h);
    let to_read = shelf_named(&h, "To read");
    assert_eq!(
        lib.shelf(fiction).and_then(|s| lib.parent_of(s)),
        Some(to_read)
    );
}

#[test]
fn f2_renames_a_row_in_place() {
    let mut h = demo();
    let id = shelf_named(&h, "To read");
    h.get_by_label("To read").focus();
    h.run();
    h.key_press(Key::F2);
    h.run_steps(3);
    assert!(h.query_by_label("Name").is_some(), "a name field");
    h.event(egui::Event::Text("Someday".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run_steps(3);
    let name = library(&h).shelf(id).map(|s| s.name.value.clone());
    assert_eq!(name.as_deref(), Some("Someday"));
    assert!(h.query_by_label("Name").is_none());
}

#[test]
fn at_four_panes_the_tree_still_moves_books_but_splits_no_more() {
    let (mut h, id, twin) = raven_and_twin("drag_only");
    let first = h.state().active_pane();
    // Right, then each half down: four roomy panes.
    use mareader_workspace::SplitAxis::{Horizontal, Vertical};
    for (axis, back) in [(Horizontal, false), (Vertical, true), (Vertical, true)] {
        h.state_mut().queue(Sent::Engine(Command::Split(axis)));
        h.run();
        if back {
            h.state_mut().queue(Sent::Engine(Command::FocusPane(first)));
            h.run();
        }
    }
    assert_eq!(h.state().panes().len(), 4);
    // Onto the reading pane's edge: refused, nothing opens.
    let rects = h.state().pane_rects().to_vec();
    let rect = rects.iter().find(|(p, _)| *p == first).unwrap().1;
    let edge = egui::pos2(rect.right() - 6.0, rect.center().y);
    let from = row(&h, "The Raven (2)").center();
    carry(&mut h, from, edge);
    release(&mut h, edge);
    assert_eq!(h.state().panes().len(), 4, "no fifth pane");
    assert_eq!(reads(&h, first), Some(id));
    // A new shelf is named in place; the tree files the twin on it.
    let new = Action::Shelf(mareader_engine::library::ShelfAction::NewIn(None));
    h.state_mut().queue(Sent::Engine(Command::Library(new)));
    h.run_steps(4);
    h.event(egui::Event::Text("Poems".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run_steps(3);
    let poems = shelf_named(&h, "Poems");
    let from = row(&h, "The Raven (2)").center();
    let to = row(&h, "Poems").center();
    carry(&mut h, from, to);
    release(&mut h, to);
    assert!(library(&h).shelves_of(twin).contains(&poems));
    assert_eq!(h.state().panes().len(), 4);
}
