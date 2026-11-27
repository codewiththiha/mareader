//! Compact previews of selection answers in Notes.

use egui::{Id, Ui};
use mareader_engine::command::Command;
use mareader_engine::library::SaveStatus;
use mareader_engine::skills::Action;

use super::SelectionRow;
use crate::ui_command::{Commands, Sent};

const PREVIEW_CHARS: usize = 180;

fn preview(source: &str) -> Option<String> {
    let mut chars = source.chars();
    let first: String = chars.by_ref().take(PREVIEW_CHARS).collect();
    chars.next().map(|_| format!("{}…", first.trim_end()))
}

pub(super) fn entry(ui: &mut Ui, answer: &SelectionRow, out: &mut Commands) {
    let state = match (answer.pinned, answer.pin_status) {
        (false, _) => "This session",
        (true, SaveStatus::Pending) => "Saving…",
        (true, SaveStatus::Failed) => "Save failed",
        (true, SaveStatus::Saved) => "Pinned",
    };
    let id = Id::new(("notes-selection-expanded", answer.id));
    let mut expanded = ui.data(|d| d.get_temp(id)).unwrap_or(false);
    let short = preview(&answer.source);
    let source = match (&short, expanded) {
        (Some(short), false) => short.as_str(),
        _ => answer.source.as_str(),
    };
    let label = format!("{} · {state}\n{source}", answer.name);
    let response = ui.add_enabled(
        short.is_some() || answer.found,
        egui::Button::new(label).wrap(),
    );
    if response.clicked() {
        if short.is_some() {
            expanded = !expanded;
            ui.data_mut(|d| d.insert_temp(id, expanded));
        } else if answer.found {
            out.push(Sent::Engine(Command::Skill(Action::Show(answer.id))));
        }
    }
    if short.is_some() {
        ui.horizontal(|ui| {
            let text = if expanded { "Show less" } else { "Show more" };
            if ui.button(text).clicked() {
                ui.data_mut(|d| d.insert_temp(id, !expanded));
            }
            if ui
                .add_enabled(answer.found, egui::Button::new("Open answer"))
                .clicked()
            {
                out.push(Sent::Engine(Command::Skill(Action::Show(answer.id))));
            }
        });
    }
    if !answer.found {
        ui.label("This passage could not be found in the current layout.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_limits_unicode_without_losing_the_source() {
        assert!(preview(&"é".repeat(PREVIEW_CHARS)).is_none());
        let source = "é".repeat(PREVIEW_CHARS + 2);
        let short = preview(&source).unwrap();
        assert_eq!(short.chars().count(), PREVIEW_CHARS + 1);
        assert!(short.ends_with('…'));
        assert_eq!(source.chars().count(), PREVIEW_CHARS + 2);
    }
    #[test]
    fn long_entry_expands_collapses_and_can_open_answer() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable as _;
        let answer = SelectionRow {
            id: mareader_engine::skills::AskId(7),
            spot: None,
            name: "Simplify".into(),
            source: format!("{} END", "a ".repeat(90)),
            pinned: false,
            pin_status: SaveStatus::Saved,
            found: true,
        };
        let mut h = Harness::new_ui_state(
            |ui, (answer, commands): &mut (SelectionRow, Commands)| entry(ui, answer, commands),
            (answer, Vec::new()),
        );
        h.run_steps(4);
        assert!(h.query_by_label_contains("END").is_none());
        h.get_by_label_contains("Simplify · This session").click();
        h.run_steps(4);
        assert!(h.query_by_label_contains("END").is_some());
        h.get_by_label("Show less").click();
        h.run_steps(4);
        assert!(h.query_by_label_contains("END").is_none());
        h.get_by_label("Open answer").click();
        h.run_steps(4);
        assert!(
            matches!(h.state().1.last(), Some(Sent::Engine(Command::Skill(Action::Show(id)))) if id.0 == 7)
        );
    }
}
