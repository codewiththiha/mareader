//! View modes, headless: what each shows, and how keys move through it.

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use mareader_engine::command::Command;
use mareader_engine::pane::ViewMode;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::{Sent, UiCommand};

type H = Harness<'static, MareaderApp>;

fn reader(pdf: &str) -> H {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("pdf", pdf).ok();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn wheel(h: &mut H, delta: egui::Vec2, ticks: usize) {
    h.hover_at(egui::pos2(450.0, 400.0));
    for _ in 0..ticks {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
    }
    h.run();
}

fn set(h: &mut H, mode: ViewMode) {
    h.state_mut().queue(Sent::Engine(Command::View(mode)));
    h.run();
}

fn press(h: &mut H, key: Key) {
    h.key_press(key);
    h.run();
}

fn mode(h: &H) -> Option<ViewMode> {
    h.state().documents().mode(h.state().active_pane())
}

fn current(h: &H) -> usize {
    let pane = h.state().active_pane();
    h.state().documents().current(pane).unwrap_or(usize::MAX)
}

fn visible(h: &H) -> std::ops::Range<usize> {
    let pane = h.state().active_pane();
    h.state().documents().visible(pane).unwrap_or(0..0)
}

#[test]
fn a_single_page_shows_alone_and_every_arrow_turns_it() {
    let mut h = reader("sample:30");
    set(&mut h, ViewMode::Single);
    assert_eq!(visible(&h), 0..1);
    press(&mut h, Key::ArrowRight);
    press(&mut h, Key::ArrowDown);
    assert_eq!((current(&h), visible(&h)), (2, 2..3));
    press(&mut h, Key::ArrowUp);
    press(&mut h, Key::H);
    assert_eq!((current(&h), visible(&h)), (0, 0..1));
    // The first page holds; nothing turns before it.
    press(&mut h, Key::ArrowLeft);
    assert_eq!(current(&h), 0);
}

#[test]
fn a_spread_pairs_pages_and_turns_by_two() {
    let mut h = reader("sample:7");
    set(&mut h, ViewMode::Spread);
    assert_eq!(visible(&h), 0..2);
    let fit = h.state().documents().zoom_state(h.state().active_pane());
    assert_eq!(
        fit.and_then(|z| z.fit),
        Some(mareader_engine::pane::Fit::Width)
    );
    press(&mut h, Key::ArrowRight);
    assert_eq!(visible(&h), 2..4);
    // Seven pages: the last spread is page 7 alone.
    for _ in 0..5 {
        press(&mut h, Key::L);
    }
    assert_eq!((current(&h), visible(&h)), (6, 6..7));
}

#[test]
fn switching_modes_keeps_the_page_being_read() {
    let mut h = reader("sample:60");
    wheel(&mut h, egui::vec2(0.0, -1000.0), 12);
    let page = current(&h);
    assert!(page > 5, "reading page {page}");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num1);
    h.run();
    assert_eq!(mode(&h), Some(ViewMode::Single));
    assert_eq!(visible(&h), page..page + 1);
    press(&mut h, Key::ArrowRight);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num2);
    h.run();
    assert_eq!(mode(&h), Some(ViewMode::Vertical));
    assert_eq!(current(&h), page + 1);
    assert_eq!(visible(&h).start, page + 1, "the column starts at it");
    set(&mut h, ViewMode::Horizontal);
    assert_eq!(visible(&h).start, page + 1);
}

#[test]
fn a_plain_wheel_runs_the_horizontal_strip_sideways() {
    let mut h = reader("sample:30");
    set(&mut h, ViewMode::Horizontal);
    // Fit page: nothing overflows down, so the wheel goes along.
    h.state_mut().queue(Sent::Engine(Command::Zoom(
        mareader_engine::pane::ZoomCommand::Fit(mareader_engine::pane::Fit::Page),
    )));
    h.run();
    assert_eq!(visible(&h).start, 0);
    wheel(&mut h, egui::vec2(0.0, -1000.0), 6);
    assert!(visible(&h).start > 3, "scrolled to {:?}", visible(&h));
    // Down the strip, the arrows are the scroll's: no turn.
    let page = current(&h);
    press(&mut h, Key::ArrowDown);
    assert_eq!(current(&h), page);
}

#[test]
fn the_menu_row_switches_modes_and_marks_the_current_one() {
    let mut h = reader("sample:10");
    h.state_mut().queue(Sent::Ui(UiCommand::ShowViewMenu));
    h.run();
    egui_kittest::kittest::Queryable::get_by_label(&h, "Two pages").click();
    h.run();
    assert_eq!(mode(&h), Some(ViewMode::Spread));
    // The menu stays open so modes can be tried in turn.
    egui_kittest::kittest::Queryable::get_by_label(&h, "Horizontal scroll").click();
    h.run();
    assert_eq!(mode(&h), Some(ViewMode::Horizontal));
}
