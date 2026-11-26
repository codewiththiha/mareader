//! Named selection-answer feedback and skill-name limits.

use crate::common::ai_reader as reader;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::skills::Anchor;
use mareader_ui_egui::ui_command::Sent;

fn select(h: &mut egui_kittest::Harness<'static, mareader_ui_egui::MareaderApp>, text: &str) {
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: text.to_owned(),
    }));
    h.run();
}

#[test]
fn new_skill_names_stop_at_24_characters_without_truncating_old_names() {
    let old = "Existing name longer than 24 characters";
    let option = old.replace(' ', "_");
    let mut h = reader(&[("pdf", "sample:5"), ("custom_skills", &option)]);
    h.state_mut().queue(Sent::Engine(Command::OpenSettings(
        mareader_engine::settings::Tab::Skills,
    )));
    h.run_steps(5);
    assert_eq!(h.state().settings().ai.tasks.skills.custom[0].name, old);
    h.get_by_role_and_label(
        egui::accesskit::Role::CheckBox,
        "Pin new answers by default",
    )
    .click();
    h.run_steps(5);
    assert_eq!(h.state().settings().ai.tasks.skills.custom[0].name, old);

    let mut h = reader(&[("pdf", "sample:5")]);
    h.state_mut().queue(Sent::Engine(Command::OpenSettings(
        mareader_engine::settings::Tab::Skills,
    )));
    h.run_steps(5);
    h.get_by_label("Add a skill").click();
    h.run_steps(5);
    h.get_by_label("Skill name").click();
    h.run_steps(2);
    h.get_by_label("Skill name").type_text(&"é".repeat(30));
    h.run_steps(5);
    let name = &h.state().settings().ai.tasks.skills.custom[0].name;
    assert_eq!(name.chars().count(), 24, "{name}");
}

#[test]
fn named_answers_at_the_same_selection_remain_independently_reachable() {
    use mareader_engine::skills::Action;
    use mareader_settings::{PageTask, Skill};
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("skill_answers", "echo"),
        ("custom_skills", "Quiz_me"),
    ]);
    let pane = h.state().active_pane();
    for skill in [
        Skill::Builtin(PageTask::Simplify),
        Skill::Custom(mareader_settings::SkillId(1)),
    ] {
        select(&mut h, "quick brown");
        h.state_mut()
            .queue(Sent::Engine(Command::Skill(Action::Run { pane, skill })));
        h.run_steps(30);
        let id = h.state().skills().asks().last().unwrap().id;
        h.state_mut()
            .queue(Sent::Engine(Command::Skill(Action::Close(id))));
        h.run_steps(20);
    }
    let a = h.get_by_label("Open Simplify answer").rect();
    let b = h.get_by_label("Open Quiz me answer").rect();
    assert!(!a.intersects(b), "{a:?} {b:?}");
    h.get_by_label("Open Quiz me answer").focus();
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(5);
    assert!(h.state().skills().asks()[1].open);
    assert!(!h.state().skills().asks()[0].open);
}

#[test]
fn selection_answer_button_stays_below_the_titlebar_after_scrolling() {
    use mareader_engine::skills::Action;
    use mareader_settings::{PageTask, Skill};
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("skill_answers", "echo"),
        ("animations", "false"),
    ]);
    select(&mut h, "quick brown");
    let pane = h.state().active_pane();
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Run {
            pane,
            skill: Skill::Builtin(PageTask::Simplify),
        })));
    h.run_steps(30);
    let ask = h.state().skills().asks().first().unwrap().id;
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Close(ask))));
    h.run_steps(10);
    let anchor = match &h.state().skills().get(ask).unwrap().anchor {
        Anchor::Text(spans) => h.state().documents().spans_rect(pane, spans).unwrap(),
        _ => panic!("not text"),
    };
    let delta = (anchor.top() - 20.0).max(0.0);
    h.hover_at(egui::pos2(450.0, 300.0));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -delta),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(60);
    let scrolled = match &h.state().skills().get(ask).unwrap().anchor {
        Anchor::Text(spans) => h.state().documents().spans_rect(pane, spans).unwrap(),
        _ => panic!("not text"),
    };
    assert!(
        scrolled.top() < 48.0 && scrolled.bottom() > 0.0,
        "{scrolled:?}"
    );
    let button = h.get_by_label("Open Simplify answer").rect();
    assert!(button.top() >= 48.0, "{button:?}");
    h.hover_at(egui::pos2(600.0, 20.0));
    h.run_steps(10);
    assert!(h.state().bar_shown());
    assert!(h.get_by_label("Open Simplify answer").rect().top() >= 48.0);
    h.get_by_label("Open Simplify answer").click();
    h.run_steps(10);
    assert!(h.state().skills().get(ask).unwrap().open);
}

#[test]
fn pane_close_uses_the_same_titlebar_clearance() {
    let mut h = reader(&[("pdf", "sample:5"), ("panes", "2"), ("animations", "false")]);
    let pane = h.state().pane_rects()[0].1;
    h.hover_at(pane.center());
    h.run_steps(8);
    let close = h.get_by_label("Close pane").rect();
    assert!(
        close.top() >= 48.0 && pane.contains_rect(close),
        "{close:?} in {pane:?}"
    );
    h.hover_at(egui::pos2(600.0, 20.0));
    h.run_steps(8);
    h.hover_at(pane.center());
    h.run_steps(2);
    assert!(h.get_by_label("Close pane").rect().top() >= 48.0);
}

#[test]
fn chromeless_pane_close_does_not_reserve_a_hidden_titlebar() {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("panes", "2"),
        ("chromeless", "true"),
        ("animations", "false"),
    ]);
    let pane = h.state().pane_rects()[0].1;
    h.hover_at(pane.center());
    h.run_steps(8);
    let close = h.get_by_label("Close pane").rect();
    assert!(
        close.top() < 48.0 && pane.contains_rect(close),
        "{close:?} in {pane:?}"
    );
}

#[test]
fn crowded_answers_offer_a_reachable_overflow_list() {
    use mareader_engine::skills::Action;
    use mareader_settings::{Skill, SkillId};
    let names = (1..=20)
        .map(|n| format!("Skill_number_{n:02}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("skill_answers", "echo"),
        ("custom_skills", &names),
        ("animations", "false"),
    ]);
    let pane = h.state().active_pane();
    for n in 1..=20 {
        select(&mut h, "quick brown");
        h.state_mut()
            .queue(Sent::Engine(Command::Skill(Action::Run {
                pane,
                skill: Skill::Custom(SkillId(n)),
            })));
        h.run_steps(10);
        let id = h.state().skills().asks().last().unwrap().id;
        h.state_mut()
            .queue(Sent::Engine(Command::Skill(Action::Close(id))));
        h.run_steps(5);
    }
    h.set_size(egui::vec2(350.0, 230.0));
    h.run_steps(10);
    let ask = h.state().skills().asks().first().unwrap();
    let anchor = match &ask.anchor {
        Anchor::Text(spans) => h.state().documents().spans_rect(pane, spans).unwrap(),
        _ => panic!("not text"),
    };
    h.hover_at(egui::pos2(180.0, 120.0));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -(anchor.top() - 100.0)),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(30);
    h.get_by_label_contains("More selection AI answers").click();
    h.run_steps(5);
    let last = h.get_by_label("Open Skill number 20 answer");
    last.scroll_to_me();
    h.run_steps(15);
    h.get_by_label("Open Skill number 20 answer").click();
    h.run_steps(5);
    assert!(h.state().skills().asks()[19].open);
}

#[test]
fn named_answers_respect_a_pinned_bar_and_split_panes() {
    use mareader_engine::skills::Action;
    use mareader_settings::{PageTask, Skill};
    for opts in [vec![("pinned", "true")], vec![("panes", "2")]] {
        let mut all = vec![
            ("pdf", "sample:5"),
            ("skill_answers", "echo"),
            ("animations", "false"),
        ];
        all.extend(opts);
        let mut h = reader(&all);
        let pane = h.state().active_pane();
        select(&mut h, "quick brown");
        h.state_mut()
            .queue(Sent::Engine(Command::Skill(Action::Run {
                pane,
                skill: Skill::Builtin(PageTask::Simplify),
            })));
        h.run_steps(30);
        let id = h.state().skills().asks().first().unwrap().id;
        h.state_mut()
            .queue(Sent::Engine(Command::Skill(Action::Close(id))));
        h.run_steps(10);
        let room = h
            .state()
            .pane_rects()
            .iter()
            .find(|(id, _)| *id == pane)
            .unwrap()
            .1;
        let rect = h.get_by_label("Open Simplify answer").rect();
        assert!(
            rect.top() >= 48.0 && room.contains_rect(rect),
            "{rect:?} in {room:?}"
        );
        if h.state().panes().len() > 1 {
            h.hover_at(room.center());
            h.run_steps(4);
            let close = h.get_by_label("Close pane").rect();
            assert!(!rect.intersects(close), "{rect:?} {close:?}");
        }
    }
}

#[test]
fn page_edge_stack_avoids_page_ai_and_hover_lights_one_answer() {
    use mareader_engine::skills::Action;
    use mareader_settings::{PageTask, Skill};
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("page_ai", "0:simplify"),
        ("skill_answers", "echo"),
        ("custom_skills", "Quiz_me"),
        ("animations", "false"),
    ]);
    let pane = h.state().active_pane();
    for (skill, text) in [
        (Skill::Builtin(PageTask::Simplify), "quick brown"),
        (Skill::Custom(mareader_settings::SkillId(1)), "lazy dog"),
    ] {
        select(&mut h, text);
        h.state_mut()
            .queue(Sent::Engine(Command::Skill(Action::Run { pane, skill })));
        h.run_steps(30);
        let id = h.state().skills().asks().last().unwrap().id;
        h.state_mut()
            .queue(Sent::Engine(Command::Skill(Action::Close(id))));
        h.run_steps(60);
    }
    let page = h.state().documents().sheets(pane)[0];
    let first = h.get_by_label("Open Simplify answer").rect();
    let second = h.get_by_label("Open Quiz me answer").rect();
    let edge = page
        .rect
        .intersect(page.shown)
        .intersect(h.state().pane_rects()[0].1);
    assert!(
        (first.right() - (edge.right() - 8.0)).abs() < 2.0,
        "{first:?} {edge:?}"
    );
    assert!(second.top() >= first.bottom(), "{first:?} {second:?}");
    let number = h.get_by_label("Simplified").rect();
    assert!(
        !first.intersects(number) && !second.intersects(number),
        "{number:?} {first:?} {second:?}"
    );
    h.hover_at(first.center());
    h.run_steps(30);
    assert_eq!(mareader_ui_egui::glow::shown(&h.ctx), 1);
    assert!(h.state().skills().asks().iter().all(|a| !a.open));
    h.hover_at(second.center());
    h.run_steps(60);
    assert_eq!(mareader_ui_egui::glow::shown(&h.ctx), 1);
    h.hover_at(egui::pos2(20.0, 300.0));
    h.run_steps(60);
    assert_eq!(mareader_ui_egui::glow::shown(&h.ctx), 0);
}
