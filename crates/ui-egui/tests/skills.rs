//! Ask AI on a selection: the skills it opens and the card that answers.

use crate::common::{ai_reader as reader, page};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::skills::{Anchor, Answer};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::activity::stack::{STACK, VIEW};
use mareader_ui_egui::skill_menu::ROW;
use mareader_ui_egui::ui_command::Sent;

type H = Harness<'static, MareaderApp>;

fn select(h: &mut H, text: &str) {
    let pane = h.state().active_pane();
    let text = text.to_owned();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text,
    }));
    h.run();
}

#[test]
fn ask_ai_opens_the_skills_then_shuts_them() {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("skill_answers", "echo"),
        ("custom_skills", "Quiz_me,Explain_the_grammar"),
    ]);
    h.run();
    select(&mut h, "quick brown");
    assert!(h.query_by_label("Translate").is_none(), "shut at first");
    h.get_by_label("Ask AI").click();
    h.run();
    for name in ["Translate", "Summarize", "Simplify", ROW] {
        assert!(h.query_by_label(name).is_some(), "{name} is listed");
    }
    assert!(h.query_by_label("Quiz me").is_none(), "folded");
    h.get_by_label("Ask AI").click();
    h.run();
    assert!(h.query_by_label(ROW).is_none(), "a second press shuts");
}

#[test]
fn typing_finds_a_skill_and_escape_folds_then_shuts() {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("skill_answers", "echo"),
        ("custom_skills", "Quiz_me,Explain_the_grammar"),
    ]);
    h.run();
    select(&mut h, "quick brown");
    h.get_by_label("Ask AI").click();
    h.run();
    h.get_by_label(ROW).click();
    h.run();
    assert!(h.query_by_label("Quiz me").is_some(), "open beside the row");
    h.event(egui::Event::Text("gram".into()));
    h.run();
    assert!(h.query_by_label("Quiz me").is_none(), "filtered out");
    assert!(h.query_by_label("Explain the grammar").is_some());
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label("Explain the grammar").is_none(), "folded");
    assert!(h.query_by_label("Translate").is_some(), "the menu stays");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(
        h.query_by_label("Translate").is_none(),
        "then the menu goes"
    );
}

#[test]
fn a_skill_answers_in_a_card_and_the_selection_goes() {
    let mut h = reader(&[("pdf", "sample:5"), ("skill_answers", "echo")]);
    h.run();
    select(&mut h, "quick brown");
    h.get_by_label("Ask AI").click();
    h.run();
    assert!(h.query_by_label("Custom skills").is_none(), "none written");
    h.get_by_label("Simplify").click();
    h.run();
    let pane = h.state().active_pane();
    assert!(h.state().documents().selection(pane).is_none());
    let asks = h.state().skills().asks();
    assert_eq!(asks.len(), 1);
    let answer = asks.first().map(|a| &a.answer);
    assert!(matches!(answer, Some(Answer::Ready(t)) if t == "quick brown"));
    assert!(h.query_by_label("Ask again").is_some(), "the card is drawn");
}

#[test]
fn an_answer_landing_out_of_sight_waits_in_the_stack_and_view_goes_back() {
    let mut h = reader(&[("pdf", "sample:12"), ("skill_answers", "echo")]);
    h.run();
    select(&mut h, "quick brown");
    let pane = h.state().active_pane();
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page: 8 }));
    h.run();
    let skill = mareader_settings::Skill::Builtin(mareader_settings::PageTask::Simplify);
    let run = mareader_engine::skills::Action::Run { pane, skill };
    h.state_mut().queue(Sent::Engine(Command::Skill(run)));
    h.run();
    assert!(h.query_by_label("Ask again").is_none(), "out of sight");
    assert!(
        h.state().toast().is_none(),
        "the stack says it, not a toast"
    );
    h.get_by_label(STACK).hover();
    h.run_steps(20);
    h.get_by_label_contains("Simplify ready");
    h.get_by_label(VIEW).click();
    h.run_steps(60);
    assert!(h.query_by_label("Ask again").is_some(), "back at its words");
    assert!(h.query_by_label(STACK).is_none(), "seen, the line goes");
}

#[test]
fn the_page_menu_folds_custom_skills_and_runs_one_on_the_page() -> Result<(), String> {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("provider", "ollama"),
        ("skill_answers", "echo"),
        ("custom_skills", "Quiz_me"),
    ]);
    h.run();
    let (_, sheet) = page(&h, 0)?;
    let at = sheet.shown.min + egui::vec2(12.0, 12.0);
    h.hover_at(at);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.run();
    assert!(h.query_by_label("Quiz me").is_none(), "folded");
    h.get_by_label(ROW).click();
    h.run();
    h.get_by_label("Quiz me").click();
    h.run();
    let asks = h.state().skills().asks();
    let spot = asks.first().map(|a| &a.anchor);
    assert!(
        matches!(spot, Some(Anchor::Spot { page: 0, .. })),
        "{spot:?}"
    );
    Ok(())
}
