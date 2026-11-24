//! The AI organizer on the library page: its run, its plan, and Undo.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::library::{Action, Heard, Paused, ShelfAction};
use mareader_engine::route::Route;
use mareader_engine::toast::Toast;
use mareader_library::{BookId, Move};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::activity::stack::STACK;
use mareader_ui_egui::ui_command::{Commands, Sent, UiCommand};
use mareader_workspace::SplitAxis;

use crate::common::without_models;

/// Frames enough for a run's card to show; its ring keeps asking for more.
const FRAMES: usize = 40;

pub(crate) fn page(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("library", "show").ok();
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|_| app);
    h.run_steps(FRAMES);
    h
}

fn act(h: &mut Harness<'static, MareaderApp>, action: Action) {
    h.state_mut().queue(Sent::Engine(Command::Library(action)));
    h.run_steps(4);
}

fn toast(h: &Harness<'static, MareaderApp>) -> String {
    let toast = h.state().toast().map(|t| t.message.clone());
    toast.unwrap_or_default()
}

fn shelved(h: &Harness<'static, MareaderApp>) -> usize {
    let model = h.state().library();
    model
        .books()
        .filter(|b| !model.shelves_of(b.id).is_empty())
        .count()
}

fn held(h: &Harness<'static, MareaderApp>) -> Vec<BookId> {
    h.state().library_state().held().iter().copied().collect()
}

#[test]
fn a_run_shows_its_card_and_its_books_stay_put_until_stopped() {
    let mut h = page(&[("copies", "5"), ("organize", "running")]);
    assert_eq!(held(&h).len(), 5);
    h.get_by_label("Pause organizing");
    let Some(book) = held(&h).first().copied() else {
        panic!("a held book")
    };
    let Ok(shelf) = h.state_mut().seed_library_shelf("Mine") else {
        panic!("a shelf")
    };
    let carry = Move {
        books: vec![book],
        shelves: Vec::new(),
        links: Vec::new(),
        from: None,
        to: Some(shelf),
        keep: false,
        before: None,
    };
    act(&mut h, Action::Shelf(ShelfAction::Carry(carry.clone())));
    assert_eq!(shelved(&h), 0, "a held book doesn't move");
    assert!(toast(&h).contains("stay put"), "{}", toast(&h));
    h.get_by_label("Pause organizing").click();
    h.run_steps(4);
    assert!(held(&h).is_empty(), "stopping lets the books go");
    assert!(h.query_by_label("Pause organizing").is_none());
    act(&mut h, Action::Shelf(ShelfAction::Carry(carry)));
    assert_eq!(shelved(&h), 1, "then it moves as any book");
}

#[test]
fn the_plan_applies_and_undo_puts_everything_back() {
    let mut h = page(&[("copies", "4"), ("organize", "review")]);
    // Three books on three new shelves, one left on no shelf.
    h.get_by_label("Classics");
    h.get_by_label("Stay on no shelf");
    h.get_by_label("Apply").click();
    h.run_steps(4);
    assert_eq!(shelved(&h), 3);
    assert_eq!(h.state().library().shelves().count(), 3);
    assert!(held(&h).is_empty());
    let said = toast(&h);
    assert!(
        said.starts_with("Filed 3 books on 3 shelves, 3 new"),
        "{said}"
    );
    h.get_by_label("Undo").click();
    h.run_steps(4);
    assert_eq!(shelved(&h), 0);
    assert_eq!(h.state().library().shelves().count(), 0);
}

#[test]
fn discarding_the_plan_moves_nothing() {
    let mut h = page(&[("copies", "4"), ("organize", "review")]);
    h.get_by_label("Discard").click();
    h.run_steps(4);
    assert!(h.query_by_label("Apply").is_none());
    assert_eq!(shelved(&h), 0);
    assert_eq!(h.state().library().shelves().count(), 0);
    assert!(held(&h).is_empty());
}

#[test]
fn the_view_menu_asks_for_a_model_before_organizing() {
    let mut h = page(&[("copies", "3")]);
    without_models(&mut h);
    h.get_by_label("View options").click();
    h.run_steps(4);
    h.get_by_label("Organize with AI").click();
    h.run_steps(4);
    let said = toast(&h);
    assert!(said.contains("Settings › AI"), "{said}");
    assert!(held(&h).is_empty());
}

#[test]
fn books_read_in_place_are_never_organized() {
    let mut h = page(&[("books", "3")]);
    act(&mut h, Action::Organize(None));
    let said = toast(&h);
    assert!(said.starts_with("Nothing to organize"), "{said}");
}

#[test]
fn a_paused_run_waits_on_its_card_with_its_books_free() {
    let mut h = page(&[("copies", "4"), ("organize", "paused")]);
    h.get_by_label("Organizing paused");
    h.get_by_label("3 of 8 steps · 4 books");
    h.get_by_label("Resume");
    assert!(held(&h).is_empty(), "paused books move as any");
    h.get_by_label("View options").click();
    h.run_steps(4);
    h.get_by_label_contains("Resume organizing");
    h.get_by_label("Organize from the start");
    assert!(h.query_by_label("Organize with AI").is_none());
    h.get_by_label("View options").click();
    h.run_steps(4);
    h.get_by_label("Discard what's done").click();
    h.run_steps(4);
    assert!(h.query_by_label("Organizing paused").is_none());
    let organizer = h.state().library_state().organizer();
    assert!(organizer.paused().is_none());
}

#[test]
fn a_new_run_while_one_is_paused_asks_first() {
    let mut h = page(&[("copies", "3"), ("organize", "paused")]);
    without_models(&mut h);
    act(&mut h, Action::Organize(None));
    h.get_by_label("Replace the paused run?");
    h.get_by_label("Cancel").click();
    h.run_steps(4);
    assert!(h.query_by_label("Replace the paused run?").is_none());
    assert!(h.state().library_state().organizer().paused().is_some());
    act(&mut h, Action::Organize(None));
    h.get_by_label("Start over").click();
    h.run_steps(4);
    assert!(h.query_by_label("Replace the paused run?").is_none());
    let said = toast(&h);
    assert!(
        said.contains("Settings › AI"),
        "a fresh run needs a model: {said}"
    );
}

#[test]
fn a_finished_plan_waits_for_review_and_a_failure_offers_resume() {
    let mut h = page(&[("copies", "3"), ("organize", "ready")]);
    h.get_by_label("Shelf plan ready");
    h.get_by_label("Review");
    h.get_by_label("Discard the plan");
    // A run that fails partway keeps its steps; its toast carries on.
    let mut out = Commands::new();
    let paused = Paused {
        done: 1,
        total: 4,
        ..Paused::default()
    };
    h.state_mut().set_library_paused(Some(paused));
    h.state_mut()
        .library_heard(Heard::Failed(Toast::error("Couldn't organize.")), &mut out);
    let Some(Sent::Ui(UiCommand::Notify(failed))) = out.first() else {
        panic!("a toast")
    };
    assert_eq!(failed.label, "Resume");
    let resume = Command::Library(Action::ResumeOrganizing);
    assert_eq!(failed.undo.as_deref(), Some(&resume));
}

fn stacks(h: &Harness<'static, MareaderApp>) -> usize {
    h.query_all_by_label(STACK).count()
}

fn send(h: &mut Harness<'static, MareaderApp>, command: Command) {
    h.state_mut().queue(Sent::Engine(command));
    h.run_steps(FRAMES);
}

#[test]
fn off_the_library_its_run_stacks_once_in_the_workspace_corner() {
    let mut h = page(&[("copies", "5"), ("organize", "running")]);
    h.get_by_label("Pause organizing");
    send(&mut h, Command::ShowRoute(Route::Reader));
    assert!(
        h.query_by_label("Pause organizing").is_none(),
        "its card stays on the library page"
    );
    assert_eq!(stacks(&h), 1, "one pane: one stack");
    send(&mut h, Command::Split(SplitAxis::Horizontal));
    assert_eq!(
        stacks(&h),
        1,
        "two panes: still one, in the workspace corner"
    );
    h.get_by_label(STACK).hover();
    h.run_steps(20);
    h.get_by_label_contains("Filing books").hover();
    h.get_by_label("Pause").click();
    h.run_steps(4);
    assert!(held(&h).is_empty(), "pausing lets the books go");
    assert!(toast(&h).starts_with("Organizing paused"), "{}", toast(&h));
    assert_eq!(stacks(&h), 0, "nothing is left in the corner");
}
