//! Drivers shared by UI integration suites.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::host::{Fake, Host, Shared};
use mareader_engine::library::Action;
use mareader_pdf::TextLayer;
use mareader_settings::{DictionaryWindow, Settings};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::selection::Sheet;
use mareader_ui_egui::ui_command::Sent;

fn harness(
    options: &[(&str, &str)],
    defaults: &[(&str, &str)],
    size: egui::Vec2,
    max_steps: u64,
) -> Harness<'static, MareaderApp> {
    built(MareaderApp::default(), options, defaults, size, max_steps)
}

/// [`reader_200`], with the engine's own two files kept in `dir`.
pub(crate) fn reader_kept(
    dir: &std::path::Path,
    options: &[(&str, &str)],
) -> Harness<'static, MareaderApp> {
    built(
        MareaderApp::kept(dir),
        options,
        &[("workers", "0")],
        egui::vec2(900.0, 700.0),
        200,
    )
}

fn built(
    mut app: MareaderApp,
    options: &[(&str, &str)],
    defaults: &[(&str, &str)],
    size: egui::Vec2,
    max_steps: u64,
) -> Harness<'static, MareaderApp> {
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

/// A host with a window's clipboard, as `apps/mareader`'s `Desk` has one.
///
/// A suite reads what the app copied from the harness's own output, so the
/// engine's fake — which records and reaches no window — is not enough where
/// a menu copies text. Build the app inside `build_eframe` to get one.
pub(crate) struct Window {
    ctx: egui::Context,
    fake: Fake,
}

impl Window {
    /// The machine for an app built in `ctx`.
    pub(crate) fn host(ctx: &egui::Context) -> Shared {
        Arc::new(Self {
            ctx: ctx.clone(),
            fake: Fake::default(),
        })
    }
}

impl Host for Window {
    fn state_dir(&self) -> Option<PathBuf> {
        self.fake.state_dir()
    }

    fn cache_dir(&self) -> Option<PathBuf> {
        self.fake.cache_dir()
    }

    fn set_clipboard(&self, text: String) {
        self.ctx.copy_text(text);
    }

    fn reveal(&self, path: &Path) -> bool {
        self.fake.reveal(path)
    }

    fn open_external(&self, path: &Path) -> bool {
        self.fake.open_external(path)
    }
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
    h.state_mut().queue(Sent::Engine(Command::Library(action)));
    h.run();
    h.run();
}

pub(crate) fn go_to(h: &mut Harness<'static, MareaderApp>, page: usize) {
    let pane = h.state().active_pane();
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page }));
    h.run();
}

/// The settings window's door, as a fixture takes it.
///
/// A suite edits a copy of the settings and sends the copy back as one
/// command, which is what the window does when a reader changes a dial. No
/// suite reaches the session's own settings, so the door stays the only way
/// in, and what a fixture writes is what the settings write.
pub(crate) trait Dials {
    /// Edits a copy of the settings, sends it, and answers what the edit did.
    fn dial<R>(&mut self, change: impl FnOnce(&mut Settings) -> R) -> R;
}

impl Dials for Harness<'_, MareaderApp> {
    fn dial<R>(&mut self, change: impl FnOnce(&mut Settings) -> R) -> R {
        let mut settings = self.state().settings().clone();
        let out = change(&mut settings);
        self.state_mut().set_settings(settings);
        out
    }
}

/// The Dictionary's window open at `rect`, as a suite places it.
pub(crate) fn window_at(rect: [f32; 4]) -> DictionaryWindow {
    DictionaryWindow {
        open: true,
        minimized: false,
        rect: Some(rect),
    }
}
