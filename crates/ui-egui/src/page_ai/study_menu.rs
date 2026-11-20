//! The page menu's study-mode switch.

use egui::Ui;
use mareader_engine::command::Command;
use mareader_engine::settings::Tab;
use mareader_engine::study_mode::Action;
use mareader_study::Mode;

use crate::icons::Icon;
use crate::ui_command::{Commands, Sent};
use crate::widgets;

/// Three ways to set the mode, and a link to its settings.
pub(super) fn rows(mode: Mode, ui: &mut Ui, out: &mut Commands) {
    widgets::menu_rule(ui);
    widgets::menu_section(ui, Some(Icon::Check), "Study mode");
    let on = |m: Mode| mode == m;
    let set = |out: &mut Commands, m: Mode| {
        out.push(Sent::Engine(Command::StudyMode(Action::Set(m))));
    };
    if widgets::menu_row(ui, Icon::Check, "Off", None, on(Mode::Off)).clicked() {
        set(out, Mode::Off);
    }
    if widgets::menu_row(ui, Icon::Book, "Proactive", None, on(Mode::Proactive)).clicked() {
        set(out, Mode::Proactive);
    }
    if widgets::menu_row(ui, Icon::Highlight, "Reactive", None, on(Mode::Reactive)).clicked() {
        set(out, Mode::Reactive);
    }
    if widgets::menu_row(ui, Icon::Settings, "Study mode settings…", None, false).clicked() {
        out.push(Sent::Engine(Command::OpenSettings(Tab::StudyMode)));
    }
}
