//! The page menu's Study section: modes, and the way to Space.

use egui::Ui;
use mareader_engine::command::Command;

use super::Menu;
use crate::icons::Icon;
use crate::ui_command::{Commands, Sent};
use crate::widgets;

/// Study mode's rows; true once one is picked.
pub(super) fn show(ui: &mut Ui, m: &Menu, out: &mut Commands) -> bool {
    if !m.pages.is_empty() {
        return false;
    }
    widgets::menu_section(ui, Some(Icon::Levels), "Study mode");
    for (mode, label) in [
        (
            mareader_study::StudyMode::Proactive,
            "Proactive — study first",
        ),
        (
            mareader_study::StudyMode::Reactive,
            "Reactive — recall on click",
        ),
        (mareader_study::StudyMode::Off, "Off"),
    ] {
        if widgets::menu_row(ui, Icon::Levels, label, None, false).clicked() {
            out.push(Sent::Engine(Command::Study(
                mareader_engine::study::Action::SetMode(m.pane, mode),
            )));
            return true;
        }
    }
    if widgets::menu_row(ui, Icon::Library, "Open Space", None, false).clicked() {
        out.push(Sent::Engine(Command::ShowRoute(
            mareader_engine::route::Route::Space,
        )));
        return true;
    }
    false
}
