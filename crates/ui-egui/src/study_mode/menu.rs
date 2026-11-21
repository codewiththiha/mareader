//! The page menu's Study section: study mode on or off, its mode, its batch.
//!
//! The app copies each pane's study state into a [`Look`] every frame,
//! before the page menu draws; the menu draws the section from it.

use egui::{Align, Layout, RichText, Ui};
use mareader_engine::command::Command;
use mareader_engine::session::Session;
use mareader_engine::settings::Tab;
use mareader_engine::study_mode::Action;
use mareader_study::mode::{Have, MAX_PAGES, Missing, Mode, Unit, check};
use mareader_workspace::PaneId;

use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;

/// Why the AI tasks are off while a pane studies.
pub const KEEPS: &str = "Study mode keeps pages as written";

/// Study mode as the page menu shows it, for every pane.
#[derive(Clone, Debug, Default)]
pub struct Look {
    /// The mode the switch turns on.
    pub mode: Mode,
    pub unit: Unit,
    pub pages: u8,
    /// Each pane's running mode, if any, and what it has.
    pub panes: Vec<(PaneId, Option<Mode>, Have)>,
}

impl Look {
    pub fn of(session: &Session, panes: &[PaneId]) -> Self {
        let settings = &session.settings().study.mode;
        let running = |pane| session.study_mode().run(pane).map(|r| r.mode);
        let pane = |&pane: &PaneId| (pane, running(pane), session.study_have(pane));
        Self {
            mode: settings.mode,
            unit: settings.unit,
            pages: settings.pages,
            panes: panes.iter().map(pane).collect(),
        }
    }
}

/// The Study section of `pane`'s page menu; true while the pane studies.
///
/// A pane missing from `look` gets no section.
pub fn show(ui: &mut Ui, look: &Look, pane: PaneId, out: &mut Commands) -> bool {
    let Some(&(_, running, have)) = look.panes.iter().find(|(p, ..)| *p == pane) else {
        return false;
    };
    widgets::menu_section(ui, Some(Icon::Levels), "Study");
    let missing = check(look.mode, have).err();
    let mut on = running.is_some();
    let can = on || missing.is_none();
    let switch = ui.add_enabled_ui(can, |ui| {
        widgets::menu_switch(ui, Icon::Levels, "Study mode", &mut on)
    });
    if switch.inner.changed() {
        let action = match on {
            true => Action::Toggle {
                pane,
                mode: look.mode,
            },
            false => Action::Off(pane),
        };
        out.push(Sent::Engine(Command::StudyMode(action)));
    }
    if let (None, Some(missing)) = (running, missing) {
        unavailable(ui, missing, out);
    }
    let was = running.unwrap_or(look.mode);
    let mut mode = was;
    let modes = [Mode::Proactive, Mode::Reactive];
    widgets::segments(ui, &mut mode, &modes, 2, |m| (None, name(m)));
    if mode != was {
        set(out, "study.mode.mode", &mode);
        if running.is_some() {
            let action = Action::Toggle { pane, mode };
            out.push(Sent::Engine(Command::StudyMode(action)));
        }
    }
    widgets::menu_note(ui, about(mode));
    if mode == Mode::Proactive && look.unit == Unit::Pages {
        batch(ui, look.pages, out);
    }
    widgets::menu_rule(ui);
    if running.is_some() {
        widgets::menu_note(ui, KEEPS);
    }
    running.is_some()
}

/// Why the switch is off, and the way to what it needs.
fn unavailable(ui: &mut Ui, missing: Missing, out: &mut Commands) {
    widgets::menu_note(ui, missing.reason());
    let Some(fix) = missing.fix() else {
        return;
    };
    if widgets::menu_row(ui, Icon::Settings, fix, None, false).clicked() {
        let tab = match missing {
            Missing::Levels => Tab::Vocabulary,
            Missing::English | Missing::Cards | Missing::Judge => Tab::Ai,
        };
        out.push(Sent::Engine(Command::OpenSettings(tab)));
    }
}

/// Pages per batch, as a stepper.
fn batch(ui: &mut Ui, pages: u8, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let most = MAX_PAGES as f32;
    ui.horizontal(|ui| {
        ui.add_space(ui.spacing().button_padding.x);
        let label = RichText::new("Batch · pages").size(t.text_small);
        ui.label(label.color(t.muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let mut n = f32::from(pages);
            let show = |v: f32| format!("{v:.0}");
            widgets::stepper(ui, &mut n, 1.0..=most, 1.0, show, "pages per batch");
            let n = n.round().clamp(1.0, most) as u8;
            if n != pages {
                set(out, "study.mode.pages", &n);
            }
        });
    });
}

/// Saves one study mode setting.
fn set(out: &mut Commands, path: &str, value: &impl serde::Serialize) {
    let Ok(value) = serde_json::to_string(value) else {
        return;
    };
    let path = path.to_owned();
    out.push(Sent::Engine(Command::SetField { path, value }));
}

fn name(mode: Mode) -> &'static str {
    match mode {
        Mode::Proactive => "Proactive",
        Mode::Reactive => "Reactive",
    }
}

/// What `mode` does, under its segments.
fn about(mode: Mode) -> &'static str {
    match mode {
        Mode::Proactive => {
            "Learn the words before you read them. The page locks at the end of \
             each batch until you've studied the next one."
        }
        Mode::Reactive => {
            "Read freely. Click a shaded word and say what it means here; the AI \
             tells you if you're right and why."
        }
    }
}
