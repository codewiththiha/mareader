//! Ask AI on a selection: the skills it opens and the card that answers.

use crate::common::ai_reader as reader;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::skills::Answer;
use mareader_ui_egui::MareaderApp;
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
        ("custom_skills", "Quiz_me"),
    ]);
    h.run();
    select(&mut h, "quick brown");
    assert!(h.query_by_label("Translate").is_none(), "shut at first");
    h.get_by_label("Ask AI").click();
    h.run();
    for name in [
        "Translate",
        "Summarize",
        "Simplify",
        "Custom skills",
        "Quiz me",
    ] {
        assert!(h.query_by_label(name).is_some(), "{name} is listed");
    }
    h.get_by_label("Ask AI").click();
    h.run();
    assert!(
        h.query_by_label("Quiz me").is_none(),
        "a second press shuts"
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
