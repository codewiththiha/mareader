//! Keys and glides: arrows, page keys, holds and page jumps, headless.

use egui::Key;
use egui_kittest::Harness;
use mareader_engine::command::Command;
use mareader_engine::pane::ViewMode;
use mareader_engine::pane::{Step, ZoomCommand};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::pdf_view::{line_step, screen_step};
use mareader_ui_egui::ui_command::Sent;

const HEIGHT: f32 = 700.0;

fn reader(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, HEIGHT))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn offset(h: &Harness<'static, MareaderApp>) -> egui::Vec2 {
    let pane = h.state().active_pane();
    h.state().documents().offset(pane).unwrap_or_default()
}

fn current(h: &Harness<'static, MareaderApp>) -> usize {
    let pane = h.state().active_pane();
    h.state().documents().current(pane).unwrap_or(0)
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.5
}

/// Edits settings as the settings window does.
fn settings(
    h: &mut Harness<'static, MareaderApp>,
    edit: impl FnOnce(&mut mareader_settings::Settings),
) {
    let mut s = h.state().settings().clone();
    edit(&mut s);
    h.state_mut()
        .queue(Sent::Engine(Command::SetSettings(Box::new(s))));
    h.run_steps(2);
}

#[test]
fn a_tap_moves_a_line_and_page_keys_nearly_a_screen() {
    let mut h = reader(&[("pdf", "sample:40")]);
    h.key_press(Key::ArrowDown);
    h.run();
    let line = line_step(HEIGHT);
    assert!(near(offset(&h).y, line), "{} after a tap", offset(&h).y);
    h.key_press(Key::J);
    h.run();
    assert!(near(offset(&h).y, 2.0 * line), "vim's j is the arrow");
    h.key_press(Key::PageDown);
    h.run();
    let screen = screen_step(HEIGHT);
    assert!(near(offset(&h).y, 2.0 * line + screen));
    h.key_press(Key::Space);
    h.run();
    assert!(near(offset(&h).y, 2.0 * line + 2.0 * screen));
    h.key_press_modifiers(egui::Modifiers::SHIFT, Key::Space);
    h.run();
    assert!(
        near(offset(&h).y, 2.0 * line + screen),
        "Shift+Space goes back"
    );
}

#[test]
fn a_key_pressed_mid_glide_adds_to_where_it_was_going() {
    let mut h = reader(&[("pdf", "sample:40")]);
    // The harness turns scroll glides off; the app's motion turns them on.
    assert!(h.ctx.global_style().scroll_animation.duration.max > 0.0);
    h.key_press(Key::PageDown);
    h.run_steps(3);
    let mid = offset(&h).y;
    assert!(mid > 0.0 && mid < screen_step(HEIGHT), "{mid} on the way");
    h.key_press(Key::PageDown);
    h.run();
    assert!(
        near(offset(&h).y, 2.0 * screen_step(HEIGHT)),
        "{}",
        offset(&h).y
    );
}

#[test]
fn a_held_arrow_glides_after_a_beat_and_stops_on_release() {
    let mut h = reader(&[("pdf", "sample:40")]);
    h.key_down(Key::ArrowDown);
    // Within the beat: only the tap's line.
    h.run_steps(15);
    let beat = offset(&h).y;
    assert!(near(beat, line_step(HEIGHT)), "{beat} before the glide");
    // A second more: the beat's last 0.1 s, then 0.9 s at 1000 pt/s.
    h.run_steps(60);
    let glided = offset(&h).y - beat;
    assert!((glided - 900.0).abs() < 20.0, "glided {glided}");
    h.key_up(Key::ArrowDown);
    h.run_steps(3);
    let stopped = offset(&h).y;
    h.run_steps(20);
    assert_eq!(offset(&h).y, stopped, "the glide ended with the key");
}

#[test]
fn across_the_column_arrows_turn_and_paged_modes_turn_on_page_keys() {
    let mut h = reader(&[("pdf", "sample:40")]);
    h.key_press(Key::ArrowRight);
    h.run();
    assert_eq!(current(&h), 1, "right turns even in the column");
    h.state_mut()
        .queue(Sent::Engine(Command::View(ViewMode::Single)));
    h.run();
    h.key_press(Key::PageDown);
    h.run();
    assert_eq!(current(&h), 2);
    h.key_press(Key::End);
    h.run();
    assert_eq!(current(&h), 39);
    h.key_press(Key::Home);
    h.run();
    assert_eq!(current(&h), 0);
}

#[test]
fn a_jump_glides_and_only_its_landing_is_drawn() {
    let mut h = reader(&[("pdf", "sample:300")]);
    let pane = h.state().active_pane();
    let (_, before) = h.state().documents().stats();
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page: 200 }));
    h.run_steps(4);
    let mid = offset(&h).y;
    assert_eq!(current(&h), 200, "the counter shows where it's going");
    h.run();
    assert!(offset(&h).y > mid, "it travelled over frames");
    let shown = h.state().documents().visible(pane).unwrap_or(0..0);
    assert!(shown.contains(&200), "landed on {shown:?}");
    // Two hundred pages flew past; none of them were drawn.
    let (_, after) = h.state().documents().stats();
    assert!(
        after <= before + shown.len() + 2,
        "{before} then {after} pages"
    );
}

#[test]
fn a_wheel_during_a_jump_takes_the_strip_back() {
    let mut h = reader(&[("pdf", "sample:300")]);
    let pane = h.state().active_pane();
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page: 200 }));
    h.run_steps(6);
    h.hover_at(egui::pos2(450.0, 400.0));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 100.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    let stopped = offset(&h).y;
    h.run_steps(30);
    assert_eq!(offset(&h).y, stopped, "it stays where the wheel left it");
    // The page read follows the scroll again, not the other way about.
    let shown = h.state().documents().visible(pane).unwrap_or(0..0);
    assert!(shown.contains(&current(&h)), "{} in {shown:?}", current(&h));
    assert!(current(&h) < 200);
}

#[test]
fn with_scroll_to_page_off_a_jump_lands_at_once() {
    let mut h = reader(&[("pdf", "sample:300")]);
    settings(&mut h, |s| s.motion.scroll_jumps = false);
    let pane = h.state().active_pane();
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page: 200 }));
    h.run_steps(3);
    let shown = h.state().documents().visible(pane).unwrap_or(0..0);
    assert!(shown.contains(&200), "already on {shown:?}");
    let landed = offset(&h).y;
    h.key_press(Key::ArrowDown);
    h.run_steps(2);
    assert!(
        near(offset(&h).y - landed, line_step(HEIGHT)),
        "a tap lands too"
    );
}

#[test]
fn with_zoom_ease_off_a_step_lands_on_its_first_frame() {
    let scale = |h: &Harness<'static, MareaderApp>| {
        let pane = h.state().active_pane();
        h.state()
            .documents()
            .sheets(pane)
            .first()
            .map_or(0.0, |s| s.scale)
    };
    let mut h = reader(&[("pdf", "sample:10")]);
    h.state_mut()
        .queue(Sent::Engine(Command::Zoom(ZoomCommand::Step(Step::In))));
    h.run_steps(2);
    assert!(
        (scale(&h) - 1.5).abs() > 0.01,
        "eased: {} on the way",
        scale(&h)
    );
    h.run();
    settings(&mut h, |s| s.motion.zoom = false);
    h.state_mut()
        .queue(Sent::Engine(Command::Zoom(ZoomCommand::Step(Step::In))));
    h.run_steps(2);
    assert!((scale(&h) - 1.75).abs() < 0.01, "landed: {}", scale(&h));
}

#[test]
fn find_opens_the_bar_which_holds_the_title_bar_until_escape() {
    use egui_kittest::kittest::Queryable as _;
    let mut h = reader(&[("pdf", "sample:20")]);
    h.hover_at(egui::pos2(450.0, 400.0));
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::F);
    h.run();
    let pane = h.state().active_pane();
    assert_eq!(h.state().find().pane(), Some(pane));
    let _ = h.get_by_label("Close search");
    // Well past the bar's grace, the open search keeps it.
    h.run_steps(60);
    assert!(h.state().bar_shown());
    // The field takes typing: a bare j scrolls nothing.
    let before = offset(&h);
    h.event(egui::Event::Text("j".into()));
    h.key_press(Key::J);
    h.run();
    assert_eq!(offset(&h), before);
    h.key_press(Key::Escape);
    h.run_steps(60);
    assert!(!h.state().find().is_open());
    assert!(!h.state().bar_shown(), "it hides again");
}

#[test]
fn find_without_a_document_opens_the_library() {
    use egui_kittest::kittest::Queryable as _;
    let mut h = reader(&[]);
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::F);
    h.run();
    assert!(!h.state().find().is_open());
    assert!(h.state().library_showing());
    let search = h.get_by_label(mareader_ui_egui::library::search::LABEL);
    assert!(search.is_focused(), "its search takes the keys");
}

#[test]
fn a_press_outside_the_find_bar_closes_it() {
    let mut h = reader(&[("pdf", "sample:5")]);
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::F);
    h.run();
    assert!(h.state().find().is_open());
    let at = egui::pos2(200.0, 500.0);
    h.hover_at(at);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.run();
    assert!(!h.state().find().is_open());
}

#[test]
fn the_search_button_toggles_the_find_bar() {
    use egui_kittest::kittest::Queryable as _;
    let mut h = reader(&[("pdf", "sample:5"), ("pinned", "true")]);
    h.get_by_label("Search").click();
    h.run();
    assert!(h.state().find().is_open());
    h.get_by_label("Search").click();
    h.run();
    assert!(!h.state().find().is_open(), "its own button closes it");
}
