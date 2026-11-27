//! Compact toast action behavior.

use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::study_mode::OPEN_CARDS;
use mareader_engine::toast::Toast;
use mareader_ui_egui::ui_command::{Sent, UiCommand};

#[test]
fn open_cards_arrow_keeps_its_name_and_sends_its_action() {
    let mut h = crate::common::ai_reader(&[("pdf", "sample:5")]);
    let pane = h.state().active_pane();
    let toast = Toast::info("Open the cards for pages 4–5? 3:31 left.")
        .action(OPEN_CARDS, Command::GoTo { pane, page: 3 });
    h.state_mut().queue(Sent::Ui(UiCommand::Notify(toast)));
    h.run_steps(15);
    let arrow = h.get_by_label(OPEN_CARDS).rect();
    assert!(arrow.width() <= 33.0, "{arrow:?}");
    h.get_by_label(OPEN_CARDS).click();
    h.run_steps(30);
    assert_eq!(h.state().documents().reading(pane).unwrap().current, 3);
    assert!(h.query_by_label(OPEN_CARDS).is_none());
}

#[test]
fn a_resume_action_keeps_its_label_and_sends_its_action() {
    let mut h = crate::common::ai_reader(&[("pdf", "sample:5")]);
    let pane = h.state().active_pane();
    let toast = Toast::error("Couldn't organize the books.")
        .action("Resume", Command::GoTo { pane, page: 3 });
    h.state_mut().queue(Sent::Ui(UiCommand::Notify(toast)));
    h.run_steps(15);
    let resume = h.get_by_label("Resume").rect();
    assert!(resume.width() > 32.0, "{resume:?}");
    h.get_by_label("Resume").click();
    h.run_steps(30);
    assert_eq!(h.state().documents().reading(pane).unwrap().current, 3);
    assert!(h.query_by_label("Resume").is_none());
}
