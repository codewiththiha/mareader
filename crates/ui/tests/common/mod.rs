//! Drivers shared by UI integration suites.

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_pdf::TextLayer;
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::library::Action;
use mareader_ui::selection::Sheet;

fn harness(
    options: &[(&str, &str)],
    defaults: &[(&str, &str)],
    size: egui::Vec2,
    max_steps: u64,
) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    for (k, v) in defaults.iter().chain(options) {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(size)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(max_steps)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

pub(crate) fn reader(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    harness(options, &[("workers", "0")], egui::vec2(900.0, 700.0), 400)
}

pub(crate) fn reader_200(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    harness(options, &[("workers", "0")], egui::vec2(900.0, 700.0), 200)
}

pub(crate) fn ai_reader(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    harness(
        options,
        &[
            ("workers", "0"),
            ("explain", "canned"),
            ("animations", "false"),
        ],
        egui::vec2(1000.0, 700.0),
        200,
    )
}

/// Page `page`'s text and where it sits on screen.
pub(crate) fn page(
    h: &Harness<'static, MareaderApp>,
    page: usize,
) -> Result<(Arc<TextLayer>, Sheet), String> {
    let pane = h.state().active_pane();
    let docs = h.state().documents();
    let reading = docs.reading(pane).ok_or("no document")?;
    let layer = reading
        .document
        .text_layer(page)
        .map_err(|e| e.to_string())?;
    let sheets = docs.sheets(pane);
    let sheet = *sheets
        .iter()
        .find(|s| s.page == page)
        .ok_or("not on screen")?;
    Ok((Arc::new(layer), sheet))
}

pub(crate) fn find(layer: &TextLayer, needle: &str) -> Result<usize, String> {
    let chars: String = layer.text.chars().iter().map(|c| c.ch).collect();
    let at = chars
        .find(needle)
        .ok_or(format!("{needle} not on the page"))?;
    Ok(chars.get(..at).unwrap_or_default().chars().count())
}

pub(crate) fn press(h: &mut Harness<'static, MareaderApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run_steps(30);
    h.get_by_label(label).click();
}

pub(crate) fn click_at(h: &mut Harness<'static, MareaderApp>, at: egui::Pos2) {
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.hover_at(at);
    h.run_steps(1);
    h.event(button(true));
    h.run_steps(1);
    h.event(button(false));
    h.run_steps(2);
}

pub(crate) fn scroll(h: &mut Harness<'static, MareaderApp>, points: f32) {
    h.hover_at((egui::vec2(900.0, 700.0) / 2.0).to_pos2());
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -points),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    h.run();
    h.run_steps(2);
}

/// Presses on `from`, moves in steps past egui's drag threshold, holds.
pub(crate) fn carry(h: &mut Harness<'static, MareaderApp>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    h.run();
    h.drag_at(from);
    h.run();
    for step in 1..=6 {
        h.hover_at(from.lerp(to, step as f32 / 6.0));
        h.run();
    }
}

pub(crate) fn release(h: &mut Harness<'static, MareaderApp>, at: egui::Pos2) {
    h.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
}

pub(crate) fn act(h: &mut Harness<'static, MareaderApp>, action: Action) {
    h.state_mut().queue(Command::Library(action));
    h.run();
    h.run();
}

pub(crate) fn go_to(h: &mut Harness<'static, MareaderApp>, page: usize) {
    let pane = h.state().active_pane();
    h.state_mut().queue(Command::GoTo { pane, page });
    h.run();
}
