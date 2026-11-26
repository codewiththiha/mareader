//! Selection menu layout and preferences.

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
fn dragging_each_edge_saves_a_global_preference() {
    use crate::common::{carry, release};
    let mut h = reader(&[("pdf", "sample:5")]);
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "quick brown".to_owned(),
    }));
    h.run_steps(30);
    for (label, delta) in [
        ("Resize menu right edge", egui::vec2(40.0, 0.0)),
        ("Resize menu left edge", egui::vec2(-25.0, 0.0)),
        ("Resize menu top edge", egui::vec2(0.0, -30.0)),
        ("Resize menu bottom edge", egui::vec2(0.0, 25.0)),
    ] {
        let before = h.state().settings().study.selection_menu;
        let from = h.get_by_label(label).rect().center();
        let to = from + delta;
        carry(&mut h, from, to);
        release(&mut h, to);
        h.run_steps(5);
        let after = h.state().settings().study.selection_menu;
        if delta.x != 0.0 {
            assert!(
                after.width > before.width,
                "{label}: {before:?} -> {after:?}"
            );
        } else {
            assert!(
                after.height > before.height,
                "{label}: {before:?} -> {after:?}"
            );
        }
    }
    let saved = h.state().settings().study.selection_menu;
    let reloaded = mareader_settings::load(&mareader_settings::to_json(h.state().settings()));
    assert_eq!(reloaded.settings.study.selection_menu, saved);
}

#[test]
fn keyboard_can_resize_and_reset_the_menu() {
    let mut h = reader(&[("pdf", "sample:5")]);
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "quick brown".to_owned(),
    }));
    h.run_steps(30);
    let initial = h.state().settings().study.selection_menu;
    h.get_by_label("Resize menu right edge").focus();
    h.run_steps(1);
    h.key_press(egui::Key::Equals);
    h.run_steps(5);
    assert_eq!(
        h.state().settings().study.selection_menu.width,
        initial.width + 8
    );
    h.key_press(egui::Key::Num0);
    h.run_steps(5);
    assert_eq!(h.state().settings().study.selection_menu, initial);
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
fn preferred_size_survives_a_temporarily_narrow_window_and_a_new_selection() {
    let mut h = reader(&[("pdf", "sample:5")]);
    h.dial(|s| {
        s.study.selection_menu = mareader_settings::SelectionMenuSize {
            width: 340,
            height: 300,
        };
        s.study.highlights.bar.clear();
    });
    h.set_size(egui::vec2(320.0, 420.0));
    h.run_steps(10);
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "quick brown".to_owned(),
    }));
    h.run_steps(30);
    let size = h.state().settings().study.selection_menu;
    assert_eq!(size.width, 340);
    assert!(h.query_by_label("Copy").is_some());
    assert!(h.query_by_label("Translate").is_some());
    h.set_size(egui::vec2(900.0, 700.0));
    h.run_steps(20);
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "lazy dog".to_owned(),
    }));
    h.run_steps(30);
    assert_eq!(h.state().settings().study.selection_menu, size);
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
fn all_action_buttons_share_their_row_center_at_each_width() {
    for width in [180, 216, 340] {
        let mut h = reader(&[("pdf", "sample:5")]);
        h.dial(|s| s.study.selection_menu.width = width);
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
                "width {width}, {bottom:?}"
            );
        }
        let row: Vec<_> = ["Note", "AI Dictionary", "Look up"]
            .iter()
            .map(|name| rect(name))
            .collect();
        for r in &row {
            assert!(
                (r.center().y - row[0].center().y).abs() < 0.1,
                "width {width}, {row:?}"
            );
        }
        assert!(row[0].left() < row[1].left() && row[1].left() < row[2].left());
    }
}
