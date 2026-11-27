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

#[test]
fn translation_hover_search_and_enter_runs_the_selected_language() {
    let mut h = reader(&[("pdf", "sample:5"), ("skill_answers", "echo")]);
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "quick brown".into(),
    }));
    h.run_steps(30);
    h.get_by_label("Translate").hover();
    h.run();
    h.event(egui::Event::Text("burm".into()));
    h.run();
    assert!(
        h.query_by_label("Language support is not verified for this model.")
            .is_none()
    );
    assert!(h.query_by_label("Burmese").is_some());
    assert!(h.query_by_label("Japanese").is_none());
    h.key_press(egui::Key::Enter);
    h.run_steps(30);
    let ask = h.state().skills().asks().last().unwrap();
    assert_eq!(ask.language(), Some("my"));
    assert_eq!(
        h.state().settings().ai.tasks.translate.language.as_deref(),
        Some("my")
    );
}

#[test]
fn escape_folds_translation_before_closing_the_selection() {
    let mut h = reader(&[("pdf", "sample:5"), ("skill_answers", "echo")]);
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "quick brown".into(),
    }));
    h.run_steps(30);
    h.get_by_label("Translate").click();
    h.run();
    h.event(egui::Event::Text("japan".into()));
    h.run();
    assert!(h.query_by_label("Japanese").is_some());
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label("Japanese").is_none());
    assert!(h.query_by_label("Translate").is_some());
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label("Translate").is_none());
}

#[test]
fn arrow_keys_choose_a_language_without_searching() {
    let mut h = reader(&[("pdf", "sample:5"), ("skill_answers", "echo")]);
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "quick brown".into(),
    }));
    h.run_steps(30);
    h.get_by_label("Translate").click();
    h.run();
    h.key_press(egui::Key::ArrowDown);
    h.run();
    h.key_press(egui::Key::Enter);
    h.run_steps(30);
    assert_eq!(
        h.state().skills().asks().last().unwrap().language(),
        Some("bn")
    );
}

#[test]
fn skill_submenus_do_not_cover_the_selection_menu() {
    for (width, text) in [
        (1000.0, "quick brown"),
        (600.0, "0123456789"),
        (320.0, "quick brown"),
    ] {
        for (row, item) in [("Translate", "Burmese"), ("Custom skills", "Quiz me")] {
            let mut h = reader(&[("pdf", "sample:5"), ("custom_skills", "Quiz_me")]);
            h.set_size(egui::vec2(width, 700.0));
            h.run_steps(10);
            let pane = h.state().active_pane();
            let doc = h.state().documents().reading(pane).unwrap().doc;
            h.state_mut().queue(Sent::Engine(Command::SelectText {
                pane,
                page: 0,
                text: text.into(),
            }));
            h.run_steps(30);
            h.get_by_label(row).hover();
            h.run_steps(10);
            let parent_id = egui::Id::new(("study-bar", doc));
            let parent = h.ctx.memory(|m| m.area_rect(parent_id)).unwrap();
            let child_point = h.get_by_label(item).rect().center();
            let layer = h.ctx.layer_id_at(child_point).unwrap();
            let child = h.ctx.memory(|m| m.area_rect(layer.id)).unwrap();
            assert!(
                child.left() >= 0.0 && child.right() <= width,
                "child outside viewport: {child:?}"
            );
            match width as u32 {
                1000 => assert!(
                    child.left() >= parent.right(),
                    "expected right: {parent:?} {child:?}"
                ),
                600 => assert!(
                    child.right() <= parent.left(),
                    "expected left: {parent:?} {child:?}"
                ),
                _ => assert!(
                    child.top() >= parent.bottom() || child.bottom() <= parent.top(),
                    "expected vertical fallback: {parent:?} {child:?}"
                ),
            }
            assert!(
                !parent.intersects(child),
                "{row}, width {width}: parent {parent:?}, child {child:?}"
            );
        }
    }
}
