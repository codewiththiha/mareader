//! Selection menu layout and preferences.

use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_settings::{BarItem, Pen};
use mareader_ui_egui::ui_command::Sent;

use crate::common::{Dials, ai_reader as reader};

#[test]
fn menu_respects_hidden_actions_and_orders_within_groups() {
    let mut h = reader(&[("pdf", "sample:5")]);
    h.dial(|s| {
        s.study.highlights.bar = vec![
            BarItem::ReadAloud,
            BarItem::Copy,
            BarItem::Note,
            BarItem::Highlight(Pen::Half),
            BarItem::Highlight(Pen::Full),
        ];
    });
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "quick brown".to_owned(),
    }));
    h.run_steps(30);
    assert!(h.query_by_label("Skills").is_none());
    assert!(h.query_by_label("Translate").is_none());
    assert!(h.query_by_label("Underline").is_none());
    assert!(h.query_by_label("AI Dictionary").is_none());
    assert!(h.query_by_label("Note").is_some());
    let read = h.get_by_label("Read aloud").rect();
    let copy = h.get_by_label("Copy").rect();
    let half = h.get_by_label("Half highlight").rect();
    let full = h.get_by_label("Highlight").rect();
    assert!(read.left() < copy.left());
    assert!(half.left() < full.left());
}

#[test]
fn narrow_pane_keeps_the_menu_and_its_actions_reachable() {
    let mut h = reader(&[("pdf", "sample:5"), ("custom_skills", "Quiz_me")]);
    h.set_size(egui::vec2(320.0, 420.0));
    h.run_steps(10);
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "quick brown".to_owned(),
    }));
    h.run_steps(40);
    assert!(h.query_by_label("Copy").is_some());
    assert!(h.query_by_label("Highlight").is_some());
    let row = h.get_by_label("Custom skills");
    row.scroll_to_me();
    h.run_steps(10);
    h.get_by_label("Custom skills").click();
    h.run_steps(10);
    assert!(h.query_by_label("Quiz me").is_some());
}
