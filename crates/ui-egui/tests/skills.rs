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

#[test]
fn an_answer_landing_out_of_sight_offers_a_toast_that_goes_back() {
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
    assert!(h.query_by_label_contains("Simplify is ready").is_some());
    h.get_by_label("Show").click();
    h.run_steps(60);
    assert!(h.query_by_label("Ask again").is_some(), "back at its words");
}
