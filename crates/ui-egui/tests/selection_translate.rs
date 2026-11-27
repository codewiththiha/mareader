//! Direct selection translation behavior.

use crate::common::ai_reader as reader;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_ui_egui::ui_command::Sent;

#[test]
fn direct_translate_uses_the_current_target_without_creating_an_override() {
    let mut h = reader(&[("pdf", "sample:5"), ("skill_answers", "echo")]);
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "quick brown".into(),
    }));
    h.run_steps(30);
    h.get_by_label("Translate").click();
    h.run_steps(30);
    assert_eq!(
        h.state().skills().asks().last().unwrap().language(),
        Some("en")
    );
    assert_eq!(h.state().settings().ai.tasks.translate.language, None);
}
