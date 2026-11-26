//! Selection menu layout and interactions.

use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_settings::{BarItem, Pen};
use mareader_ui_egui::ui_command::Sent;

use crate::common::{Dials, ai_reader as reader};

#[test]
fn old_customizations_no_longer_hide_or_reorder_actions() {
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
    assert!(h.query_by_label("Skills").is_some());
    assert!(h.query_by_label("Translate").is_some());
    assert!(h.query_by_label("Underline").is_some());
    assert!(h.query_by_label("AI Dictionary").is_some());
    assert!(h.query_by_label("Note").is_some());
    let read = h.get_by_label("Read aloud").rect();
    let copy = h.get_by_label("Copy").rect();
    let half = h.get_by_label("Half highlight").rect();
    let full = h.get_by_label("Highlight").rect();
    assert!(copy.left() < read.left());
    assert!(full.left() < half.left());
    assert!(h.get_by_label("Translate").rect().bottom() < copy.top());
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

#[test]
fn legacy_sizes_are_ignored_and_resize_controls_are_gone() {
    let mut h = reader(&[("pdf", "sample:5")]);
    let mut json: serde_json::Value =
        serde_json::from_str(&mareader_settings::to_json(h.state().settings())).unwrap();
    json["study"]["selection_menu"] = serde_json::json!({"width": 340, "height": 480});
    let loaded = mareader_settings::load(&json.to_string());
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
    h.state_mut().set_settings(loaded.settings);
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "quick brown".to_owned(),
    }));
    h.run_steps(30);
    let translate = h.get_by_label("Translate").rect();
    assert!((translate.width() - 180.0).abs() < 1.0, "{translate:?}");
    for label in [
        "Resize menu right edge",
        "Resize menu left edge",
        "Resize menu top edge",
        "Resize menu bottom edge",
    ] {
        assert!(h.query_by_label(label).is_none());
    }
    assert!(h.query_by_label("Copy").is_some());
    assert!(h.query_by_label("Note").is_some());
    assert!(h.query_by_label("AI Dictionary").is_some());
    assert!(!mareader_settings::to_json(h.state().settings()).contains("selection_menu"));
    let start = egui::pos2(translate.right() + 4.0, translate.center().y);
    let end = start + egui::vec2(80.0, 0.0);
    crate::common::carry(&mut h, start, end);
    crate::common::release(&mut h, end);
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "lazy dog".to_owned(),
    }));
    h.run_steps(30);
    assert!((h.get_by_label("Translate").rect().width() - 180.0).abs() < 1.0);
}

#[test]
fn bottom_actions_are_evenly_spaced() {
    let mut h = reader(&[("pdf", "sample:5")]);
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "quick brown".to_owned(),
    }));
    h.run_steps(30);
    let centers: Vec<f32> = [
        "Copy",
        "Read aloud",
        "Highlight",
        "Half highlight",
        "Underline",
    ]
    .iter()
    .map(|label| h.get_by_label(label).rect().center().x)
    .collect();
    let gaps: Vec<f32> = centers.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(
        gaps.iter().all(|g| (g - gaps[0]).abs() < 1.0),
        "{centers:?}"
    );
}

#[test]
fn empty_custom_skills_submenu_explains_its_state() {
    let mut h = reader(&[("pdf", "sample:5")]);
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "quick brown".to_owned(),
    }));
    h.run_steps(30);
    h.get_by_label("Custom skills").click();
    h.run_steps(10);
    assert!(h.query_by_label("No custom skills yet").is_some());
}

#[test]
fn all_action_buttons_share_their_row_center_in_the_fixed_menu() {
    for viewport in [320.0, 900.0] {
        let mut h = reader(&[("pdf", "sample:5")]);
        h.set_size(egui::vec2(viewport, 700.0));
        h.run_steps(10);
        let pane = h.state().active_pane();
        h.state_mut().queue(Sent::Engine(Command::SelectText {
            pane,
            page: 0,
            text: "quick brown".to_owned(),
        }));
        h.run_steps(30);
        let rect = |name| h.get_by_label(name).rect();
        let bottom: Vec<_> = [
            "Copy",
            "Read aloud",
            "Highlight",
            "Half highlight",
            "Underline",
        ]
        .iter()
        .map(|name| rect(name))
        .collect();
        for r in &bottom {
            assert!(
                (r.center().y - bottom[0].center().y).abs() < 0.1,
                "viewport {viewport}, {bottom:?}"
            );
        }
        let row: Vec<_> = ["Note", "AI Dictionary", "Look up"]
            .iter()
            .map(|name| rect(name))
            .collect();
        for r in &row {
            assert!(
                (r.center().y - row[0].center().y).abs() < 0.1,
                "viewport {viewport}, {row:?}"
            );
        }
        assert!(row[0].left() < row[1].left() && row[1].left() < row[2].left());
    }
}
