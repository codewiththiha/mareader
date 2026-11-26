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
fn selection_menu_shows_skills_and_folds_custom_ones() {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("skill_answers", "echo"),
        ("custom_skills", "Quiz_me,Explain_the_grammar"),
    ]);
    h.run();
    select(&mut h, "quick brown");
    for name in ["Translate", "Summarize", "Simplify", ROW, "Manage skills"] {
        assert!(h.query_by_label(name).is_some(), "{name} is listed");
    }
    assert!(h.query_by_label("Quiz me").is_none(), "folded");
}

#[test]
fn managing_skills_opens_the_existing_settings() {
    let mut h = reader(&[("pdf", "sample:5")]);
    select(&mut h, "quick brown");
    h.get_by_label("Manage skills").click();
    h.run_steps(10);
    assert!(h.state().settings_window().is_open());
    assert_eq!(
        h.state().settings_window().tab(),
        mareader_engine::settings::Tab::Skills
    );
    assert!(h.query_by_label("Add a skill").is_some());
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
fn enter_runs_the_first_skill_found() {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("skill_answers", "echo"),
        ("custom_skills", "Quiz_me,Explain_the_grammar"),
    ]);
    h.run();
    select(&mut h, "quick brown");
    h.get_by_label(ROW).click();
    h.run();
    // Both hold an e: the first is lit.
    h.event(egui::Event::Text("e".into()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run_steps(30);
    let asks = h.state().skills().asks();
    let names: Vec<&str> = asks.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Quiz me"], "the first match runs");
}

#[test]
fn a_skill_answers_in_a_card_and_the_selection_goes() {
    let mut h = reader(&[("pdf", "sample:5"), ("skill_answers", "echo")]);
    h.run();
    select(&mut h, "quick brown");
    assert!(
        h.query_by_label("Custom skills").is_some(),
        "the empty list remains discoverable"
    );
    h.get_by_label("Simplify").click();
    h.run_steps(30);
    let pane = h.state().active_pane();
    assert!(h.state().documents().selection(pane).is_none());
    let asks = h.state().skills().asks();
    assert_eq!(asks.len(), 1);
    assert!(matches!(
        asks[0].spot,
        Some(mareader_library::Spot::Chars { .. })
    ));
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

#[test]
fn running_selection_hands_feedback_to_the_corner_when_away() {
    use mareader_engine::activity::Key;
    use mareader_engine::skills::Action;
    use mareader_settings::{PageTask, Skill};
    use mareader_ui_egui::activity::seen;

    let mut h = reader(&[("pdf", "sample:12"), ("skill_answers", "silent")]);
    h.run();
    select(&mut h, "quick brown");
    let pane = h.state().active_pane();
    let skill = Skill::Builtin(PageTask::Simplify);
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Run { pane, skill })));
    h.run_steps(30);
    let ask = h.state().skills().asks().first().expect("the ask").id;
    let key = Key::Skill { pane, ask };
    assert!(seen::shown(&h.ctx, key), "the outline is in view");
    assert!(
        mareader_ui_egui::glow::shown(&h.ctx) > 0,
        "the shared AI flare is painted"
    );
    assert!(
        h.query_by_label(STACK).is_none(),
        "no duplicate corner ring"
    );
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page: 8 }));
    h.run_steps(40);
    assert!(!seen::shown(&h.ctx, key), "the outline is away");
    assert_eq!(
        mareader_ui_egui::glow::shown(&h.ctx),
        0,
        "no off-screen flare"
    );
    assert!(h.query_by_label(STACK).is_some(), "the corner takes over");
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page: 0 }));
    h.run_steps(40);
    assert!(seen::shown(&h.ctx, key), "back at the outline");
    assert!(h.query_by_label(STACK).is_none(), "the corner clears");
    h.state_mut()
        .set_option("skill_answers", "echo")
        .expect("scripted answer");
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Retry(ask))));
    h.run_steps(30);
    assert!(
        matches!(
            h.state().skills().get(ask).map(|a| &a.answer),
            Some(Answer::Ready(_))
        ),
        "retry completed"
    );
    assert!(
        mareader_ui_egui::glow::shown(&h.ctx) > 0,
        "the focused flare stays"
    );
    h.state_mut()
        .set_option("skill_answers", "silent")
        .expect("silent answer");
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Retry(ask))));
    h.run_steps(30);
    assert!(seen::shown(&h.ctx, key), "retry restores the outline");
    assert!(
        mareader_ui_egui::glow::shown(&h.ctx) > 0,
        "retry restores the flare"
    );
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Close(ask))));
    h.run_steps(10);
    assert!(
        seen::shown(&h.ctx, key),
        "closing does not stop the working outline"
    );
    assert!(mareader_ui_egui::glow::shown(&h.ctx) > 0);
}

#[test]
fn a_paragraph_in_a_reflowed_book_has_a_running_outline() {
    use mareader_engine::activity::Key;
    use mareader_engine::skills::Action;
    use mareader_settings::{PageTask, Skill};
    use mareader_ui_egui::activity::seen;

    let mut h = reader(&[("epub", "sample:3"), ("skill_answers", "silent")]);
    let pane = h.state().active_pane();
    let doc = h.state().documents().reading(pane).expect("book").document;
    let (number, text) = (0..doc.page_count())
        .find_map(|number| {
            let layer = doc.text_layer(number).ok()?;
            let chars: Vec<char> = layer.text.chars().iter().map(|c| c.ch).collect();
            let start = chars
                .windows(5)
                .position(|w| w == ['r', 'i', 'v', 'e', 'r'])?;
            let text: String = chars.iter().skip(start).take(140).collect();
            Some((number, text))
        })
        .expect("prose page");
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page: number }));
    h.run_steps(30);
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: number,
        text,
    }));
    h.run_steps(30);
    let selected = h
        .state()
        .documents()
        .selection(pane)
        .expect("paragraph selected");
    let (layer, _) = page(&h, number).expect("page laid out");
    let range = selected
        .on_page(number, layer.text.len())
        .expect("selection on page");
    let boxes: Vec<_> = layer.text.line_boxes(range).collect();
    assert!(
        boxes.len() > 1,
        "paragraph covers multiple lines: {boxes:?}"
    );
    let skill = Skill::Builtin(PageTask::Simplify);
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Run { pane, skill })));
    h.run_steps(30);
    let entry = h.state().skills().asks().first().expect("the ask");
    assert!(matches!(
        entry.spot,
        Some(mareader_library::Spot::Blocks { .. })
    ));
    let ask = entry.id;
    assert!(
        seen::shown(&h.ctx, Key::Skill { pane, ask }),
        "the paragraph outline is in view"
    );
}

#[test]
fn selection_focus_stays_on_completion_and_close_plays_the_outro() {
    use mareader_engine::skills::Action;
    use mareader_settings::{PageTask, Skill};
    use mareader_ui_egui::glow;

    let mut h = reader(&[
        ("pdf", "sample:12"),
        ("skill_answers", "silent"),
        ("animations", "true"),
    ]);
    select(&mut h, "quick brown");
    let pane = h.state().active_pane();
    let skill = Skill::Builtin(PageTask::Simplify);
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Run { pane, skill })));
    h.run_steps(60);
    let ask = h.state().skills().asks().first().unwrap().id;
    h.state_mut().set_option("skill_answers", "echo").unwrap();
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Retry(ask))));
    h.run_steps(60);
    assert!(h.state().skills().get(ask).unwrap().answer.landed());
    assert!(glow::shown(&h.ctx) > 0, "the focused flare continues");
    let id = egui::Id::new(("skill-glow", ask.0));
    let focused = h
        .ctx
        .animate_bool_with_time(id.with("focus-whole"), true, 0.3);
    assert!(focused > 0.99, "the whole outline lifts into focus");
    let held = glow::state(&h.ctx, id, true).expect("focus stays bright");
    assert!(held.strength > 0.99, "focus does not fade out");
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Close(ask))));
    h.run_steps(6);
    assert!(!h.state().skills().get(ask).unwrap().open);
    let focused_outro = h
        .ctx
        .animate_bool_with_time(id.with("focus-whole"), true, 0.3);
    assert!(
        focused_outro > 0.99,
        "closing keeps the whole outline lit while it fades"
    );
    let fading = glow::state(&h.ctx, id, false).expect("close still glows");
    assert!(fading.whole > 0.0 && fading.strength > 0.0);
    h.run_steps(60);
    assert_eq!(glow::shown(&h.ctx), 0);
    h.get_by_label("Open selection AI answer").click();
    h.run_steps(60);
    assert!(h.state().skills().get(ask).unwrap().open);
    assert!(glow::shown(&h.ctx) > 0);
}

#[test]
fn closing_a_working_selection_keeps_it_running() {
    use mareader_engine::skills::Action;
    use mareader_settings::{PageTask, Skill};
    let mut h = reader(&[("pdf", "sample:12"), ("skill_answers", "silent")]);
    select(&mut h, "quick brown");
    let pane = h.state().active_pane();
    let skill = Skill::Builtin(PageTask::Simplify);
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Run { pane, skill })));
    h.run_steps(30);
    let ask = h.state().skills().asks().first().unwrap().id;
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Close(ask))));
    h.run_steps(30);
    assert!(h.state().skills().running());
    assert!(!h.state().skills().get(ask).unwrap().open);
    assert!(mareader_ui_egui::glow::shown(&h.ctx) > 0);
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Cancel(ask))));
    h.run_steps(30);
    assert!(h.state().skills().get(ask).is_none());
}

#[test]
fn selection_answers_appear_in_annotations_and_reopen() {
    use mareader_engine::skills::Action;
    use mareader_settings::{PageTask, Skill};
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("skill_answers", "echo"),
        ("sidebar", "notes"),
    ]);
    select(&mut h, "quick brown");
    let pane = h.state().active_pane();
    let skill = Skill::Builtin(PageTask::Simplify);
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Run { pane, skill })));
    h.run_steps(30);
    let ask = h.state().skills().asks().first().unwrap().id;
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Close(ask))));
    h.run_steps(60);
    h.get_by_label("Selection AI · 1").click();
    h.run_steps(10);
    h.get_by_label_contains("Simplify · This session").click();
    h.run_steps(30);
    assert!(h.state().skills().get(ask).unwrap().open);
}

#[test]
fn a_closed_working_answer_flares_out_only_after_it_lands() {
    use mareader_engine::skills::Action;
    use mareader_settings::{PageTask, Skill};
    use mareader_ui_egui::glow;
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("skill_answers", "silent"),
        ("animations", "true"),
    ]);
    select(&mut h, "quick brown");
    let pane = h.state().active_pane();
    let skill = Skill::Builtin(PageTask::Simplify);
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Run { pane, skill })));
    h.run_steps(60);
    let ask = h.state().skills().asks().first().unwrap().id;
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Close(ask))));
    h.run_steps(30);
    assert!(glow::shown(&h.ctx) > 0);
    assert!(h.query_by_label("Open selection AI answer").is_none());
    h.state_mut().set_option("skill_answers", "echo").unwrap();
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Retry(ask))));
    h.run_steps(6);
    let id = egui::Id::new(("skill-glow", ask.0));
    let fading = glow::state(&h.ctx, id, false).unwrap();
    assert!(fading.strength > 0.0 && fading.whole > 0.0);
    h.run_steps(60);
    assert_eq!(glow::shown(&h.ctx), 0);
    h.get_by_label("Open selection AI answer");
}

#[test]
fn a_failed_open_answer_keeps_its_focused_flare() {
    use mareader_engine::skills::Action;
    use mareader_settings::{PageTask, Skill};
    let mut h = reader(&[("pdf", "sample:5"), ("skill_answers", "failing")]);
    select(&mut h, "quick brown");
    let pane = h.state().active_pane();
    let skill = Skill::Builtin(PageTask::Simplify);
    h.state_mut()
        .queue(Sent::Engine(Command::Skill(Action::Run { pane, skill })));
    h.run_steps(60);
    let ask = h.state().skills().asks().first().unwrap();
    assert!(matches!(ask.answer, Answer::Failed(_)));
    assert!(ask.open);
    assert!(mareader_ui_egui::glow::shown(&h.ctx) > 0);
}
