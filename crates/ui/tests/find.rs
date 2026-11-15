//! Searching a document from the find bar: counts, steps and reveals.

use egui::Key;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;

use crate::common::{click_at as press, reader};

/// Opens the bar, types `query` and waits out the pause.
fn search(h: &mut Harness<'static, MareaderApp>, query: &str) {
    h.hover_at(egui::pos2(450.0, 400.0));
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::F);
    h.run_steps(2);
    h.get_by_role(egui::accesskit::Role::TextInput)
        .type_text(query);
    h.run_steps(20);
}

fn enter(h: &mut Harness<'static, MareaderApp>, back: bool) {
    let shift = if back {
        egui::Modifiers::SHIFT
    } else {
        egui::Modifiers::NONE
    };
    h.key_press_modifiers(shift, Key::Enter);
    h.run_steps(40);
}

/// Hit `i` was drawn last frame, clear of the title bar.
fn assert_shown(h: &Harness<'static, MareaderApp>, i: usize) {
    let pane = h.state().active_pane();
    let hit = h.state().found().and_then(|f| f.hits.get(i).cloned());
    let Some(hit) = hit else {
        panic!("no hit {i}");
    };
    let shown = h
        .state()
        .documents()
        .hit_box(pane, hit.page, hit.chars.clone());
    let offset = h.state().documents().offset(pane);
    let seen = format!("hit {i} on {} box {shown:?} at {offset:?}", hit.page);
    let inside = shown.is_some_and(|r| r.top() >= 56.0 && r.bottom() <= 700.0);
    assert!(inside, "{seen}");
}

fn current(h: &Harness<'static, MareaderApp>) -> usize {
    let pane = h.state().active_pane();
    h.state().documents().current(pane).unwrap_or(0)
}

fn hit_pages(h: &Harness<'static, MareaderApp>) -> Vec<usize> {
    let found = h.state().found();
    found
        .map(|f| f.hits.iter().map(|hit| hit.page).collect())
        .unwrap_or_default()
}

#[test]
fn a_query_counts_every_page_and_enter_steps_through_them() {
    let mut h = reader(&[("pdf", "sample:5")]);
    search(&mut h, "QUICK");
    assert_eq!(hit_pages(&h), vec![0, 1, 2, 3, 4]);
    let _ = h.get_by_label("5 found");
    enter(&mut h, false);
    assert_eq!(h.state().find().active(), Some(0));
    let _ = h.get_by_label("1 of 5");
    enter(&mut h, false);
    assert_eq!(h.state().find().active(), Some(1));
    assert_eq!(current(&h), 1, "the second hit's page is brought in");
    assert_shown(&h, 1);
    enter(&mut h, true);
    enter(&mut h, true);
    assert_eq!(h.state().find().active(), Some(4), "stepping back wraps");
    assert_eq!(current(&h), 4);
}

#[test]
fn a_match_runs_across_lines_and_misses_count_zero() {
    let mut h = reader(&[("pdf", "sample:3")]);
    search(&mut h, "0123456789 sphinx");
    assert_eq!(hit_pages(&h), vec![0, 1, 2]);
    let snippet = h
        .state()
        .found()
        .and_then(|f| f.hits.first())
        .map(|f| f.snippet.clone());
    assert!(snippet.unwrap_or_default().contains("Sphinx of black"));
    h.get_by_role(egui::accesskit::Role::TextInput)
        .type_text("zz");
    h.run_steps(20);
    let _ = h.get_by_label("No matches");
}

#[test]
fn the_first_enter_starts_from_the_page_read() {
    let mut h = reader(&[("pdf", "sample:6")]);
    let pane = h.state().active_pane();
    h.state_mut().queue(Command::GoTo { pane, page: 3 });
    h.run_steps(30);
    assert_eq!(current(&h), 3);
    search(&mut h, "quick");
    enter(&mut h, false);
    assert_eq!(h.state().find().active(), Some(3));
    assert_eq!(current(&h), 3);
}

#[test]
fn the_results_list_goes_to_a_picked_hit() {
    let mut h = reader(&[("pdf", "sample:5")]);
    search(&mut h, "quick");
    h.get_by_label("Show results").click();
    h.run_steps(3);
    let row = h.get_by_label_contains("Page 3: ").rect();
    let bar = h.state().find().rect().unwrap_or(egui::Rect::NOTHING);
    assert!(
        bar.contains_rect(row),
        "the list shows its rows: {bar:?} {row:?}"
    );
    press(&mut h, row.center());
    assert!(
        h.state().find().is_open(),
        "a press on the list keeps the bar"
    );
    assert!(
        h.query_by_label("Show results").is_some(),
        "a pick folds the list"
    );
    h.run_steps(40);
    assert_eq!(h.state().find().active(), Some(2));
    assert_shown(&h, 2);
}

#[test]
fn a_book_is_searched_and_its_hit_revealed() {
    let mut h = reader(&[("epub", "sample")]);
    search(&mut h, "Bold");
    let pages = hit_pages(&h);
    assert!(pages.len() > 12, "every paragraph has one: {}", pages.len());
    enter(&mut h, false);
    let active = h.state().find().active().unwrap_or(usize::MAX);
    assert_shown(&h, active);
    enter(&mut h, true);
    let last = pages.len() - 1;
    assert_eq!(h.state().find().active(), Some(last), "stepping back wraps");
    assert_shown(&h, last);
}

#[test]
fn a_book_set_again_or_closed_lets_its_old_search_go() {
    // The lone pane stays; a second one can close.
    let mut h = reader(&[("panes", "2"), ("focus", "2"), ("epub", "sample")]);
    search(&mut h, "Bold");
    assert_eq!(h.state().searches(), 1);
    // Set in another size, the book is a new document, searched afresh.
    h.state_mut().set_option("text_size", "26").ok();
    h.run_steps(20);
    assert_eq!(h.state().searches(), 1, "only the book as set now");
    assert!(!hit_pages(&h).is_empty(), "found again");
    let pane = h.state().active_pane();
    assert_eq!(h.state().panes().get(1), Some(&pane), "read in the second");
    h.state_mut().queue(Command::ClosePane(pane));
    h.run_steps(3);
    assert_eq!(h.state().panes().len(), 1);
    assert_eq!(h.state().searches(), 0, "closed, nothing holds the book");
}

#[test]
fn a_hit_below_a_tall_figure_is_scrolled_into_view() {
    let mut h = reader(&[("epub", "sample")]);
    search(&mut h, "river at dusk");
    enter(&mut h, false);
    assert_shown(&h, 0);
}

#[test]
fn escape_closes_the_bar_and_keeps_the_query() {
    let mut h = reader(&[("pdf", "sample:3")]);
    search(&mut h, "quick");
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(!h.state().find().is_open());
    assert_eq!(h.state().find().query(), "quick");
}

#[test]
fn a_page_picked_on_the_map_scrolls_the_list_to_its_row() {
    let mut h = reader(&[("pdf", "sample:30")]);
    search(&mut h, "quick");
    h.get_by_label("Show results").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("Page 25: ").is_none());
    h.get_by_label(&mareader_ui::find_bar::tick_name(25))
        .click();
    h.run_steps(3);
    assert_eq!(h.state().find().active(), Some(24));
    let row = h.get_by_label_contains("Page 25: ").rect();
    let bar = h.state().find().rect().unwrap_or(egui::Rect::NOTHING);
    assert!(bar.contains_rect(row), "row in view: {bar:?} {row:?}");
}

#[test]
fn the_list_before_a_query_hints_instead_of_searching() {
    let mut h = reader(&[("pdf", "sample:3")]);
    search(&mut h, "");
    h.get_by_label("Show results").click();
    h.run_steps(3);
    let _ = h.get_by_label("Type a word to see every place it appears");
    assert!(h.query_by_label("Searching…").is_none());
}
