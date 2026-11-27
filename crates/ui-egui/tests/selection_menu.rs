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
fn skill_submenus_stay_beside_the_menu_when_possible() {
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
                _ => {
                    let trigger = h.get_by_label(row).rect();
                    assert!(
                        child.top() <= trigger.center().y && child.bottom() >= trigger.center().y,
                        "submenu must be reachable horizontally: {trigger:?} {child:?}"
                    );
                    assert!(parent.intersects(child), "narrow screen permits overlap");
                    let into = egui::pos2(
                        (child.left() + 8.0).max(trigger.center().x),
                        trigger.center().y,
                    );
                    h.hover_at(into);
                    h.run_steps(30);
                    assert!(
                        h.query_by_label(item).is_some(),
                        "{row} should stay open across overlap"
                    );
                    let overlap = child.intersect(parent);
                    if overlap.is_positive() {
                        for y in [
                            overlap.top() + 4.0,
                            overlap.center().y,
                            overlap.bottom() - 4.0,
                        ] {
                            h.hover_at(egui::pos2(overlap.center().x, y));
                            h.run_steps(30);
                            assert!(
                                h.query_by_label(item).is_some(),
                                "{row} closed inside its popup"
                            );
                            let other = if row == "Translate" {
                                "Quiz me"
                            } else {
                                "Burmese"
                            };
                            assert!(
                                h.query_by_label(other).is_none(),
                                "another submenu opened through {row}"
                            );
                        }
                    }
                }
            }
            if width > 320.0 {
                assert!(
                    !parent.intersects(child),
                    "{row}, width {width}: parent {parent:?}, child {child:?}"
                );
            }
        }
    }
}

#[test]
fn answer_header_controls_share_size_and_alignment() {
    let mut h = reader(&[("pdf", "sample:5"), ("skill_answers", "echo")]);
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "quick brown".into(),
    }));
    h.run_steps(10);
    h.get_by_label("Simplify").click();
    h.run_steps(30);
    let remove = h.get_by_label("Remove answer").rect();
    let pin = h.get_by_label("Pin answer").rect();
    let close = h.get_by_label("Close").rect();
    assert_eq!(remove.size(), pin.size());
    assert_eq!(pin.size(), close.size());
    assert!((remove.center().y - pin.center().y).abs() < 1.0);
    assert!((close.center().y - pin.center().y).abs() < 1.0);
}

#[test]
fn common_actions_remain_nearest_the_selection_on_either_side() {
    for (text, below) in [("quick brown", false), ("Page 1 of 5", true)] {
        let mut h = reader(&[("pdf", "sample:5")]);
        let pane = h.state().active_pane();
        let doc = h.state().documents().reading(pane).unwrap().doc;
        h.state_mut().queue(Sent::Engine(Command::SelectText {
            pane,
            page: 0,
            text: text.into(),
        }));
        h.run_steps(40);
        let id = egui::Id::new(("study-bar", doc));
        let side = mareader_ui_egui::bubble::Bubble::side_of(&h.ctx, id).unwrap();
        assert_eq!(
            side == mareader_ui_egui::bubble::Side::Below,
            below,
            "{text}"
        );
        let copy = h.get_by_label("Copy").rect();
        let note = h.get_by_label("Note").rect();
        let skills = h.get_by_label("Skills").rect();
        if below {
            assert!(copy.bottom() < note.top() && note.bottom() < skills.top());
        } else {
            assert!(skills.bottom() < note.top() && note.bottom() < copy.top());
        }
    }
}

#[test]
fn notes_preview_expands_inline_and_jump_can_be_undone() -> Result<(), String> {
    use mareader_engine::skills::Action;
    use mareader_settings::{PageTask, Skill};
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("sidebar", "notes"),
        ("skill_answers", "echo"),
    ]);
    let pane = h.state().active_pane();
    let (layer, _) = crate::common::page(&h, 0)?;
    let source: String = layer
        .text
        .chars()
        .iter()
        .map(|c| c.ch)
        .skip(30)
        .take(240)
        .collect();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: source,
    }));
    h.run_steps(10);
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Run {
            pane,
            skill: Skill::Builtin(PageTask::Simplify),
        })));
    h.run_steps(30);
    let ask = h
        .state()
        .skills()
        .asks()
        .first()
        .ok_or("selection did not run")?
        .id;
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Close(ask))));
    h.run_steps(10);
    crate::common::go_to(&mut h, 3);
    h.run_steps(20);
    assert_eq!(h.state().documents().reading(pane).unwrap().current, 3);
    let collapsed = h.get_by_label_contains("Simplify · This session").rect();
    let more = h.get_by_label("Show more").rect();
    assert!(
        collapsed.contains_rect(more),
        "link must sit in preview: {collapsed:?} {more:?}"
    );
    h.get_by_label("Show more").click();
    h.run_steps(10);
    assert_eq!(h.state().documents().reading(pane).unwrap().current, 3);
    assert!(!h.state().skills().get(ask).unwrap().open);
    h.get_by_label("Show less").click();
    h.run_steps(5);
    h.get_by_label_contains("Simplify · This session").click();
    h.run_steps(30);
    assert_eq!(h.state().documents().reading(pane).unwrap().current, 0);
    assert!(h.state().skills().get(ask).unwrap().open);
    assert!(
        h.state()
            .toast()
            .is_some_and(|t| t.message.contains("page 4"))
    );
    h.get_by_label("Undo").click();
    h.run_steps(30);
    assert_eq!(h.state().documents().reading(pane).unwrap().current, 3);
    Ok(())
}
