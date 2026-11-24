//! Reactive study mode's recall, headless: a shaded word clicked, answered
//! wrong then right, a judge that fails, an answer revealed, a judge in the
//! AI stack, and the Study mode settings tab.

use std::time::Duration;

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::settings::Tab;
use mareader_engine::study_mode::Recalling;
use mareader_study::mode::{Mode, Outcome, Phase};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::activity::stack::STACK;
use mareader_ui_egui::study_mode::recall::{AGAIN, CANCEL, CHECK, GOT_IT, MEMORIZED, UNSURE};
use mareader_ui_egui::ui_command::Sent;
use mareader_workspace::PaneId;

use crate::common::{click, levels_reader, shelves};

type H = Harness<'static, MareaderApp>;
type Shelves = (tempfile::TempDir, tempfile::TempDir);

/// A reader in Reactive mode, a scripted AI writing backs and judging.
fn recalling(script: &str) -> (H, PaneId, Shelves) {
    let (words, packs) = shelves();
    let mut h = levels_reader(words.path(), Some(packs.path()));
    h.state_mut().set_option("study_cards", script).unwrap();
    h.state_mut().set_option("study_mode", "reactive").unwrap();
    h.run_steps(4);
    let pane = h.state().active_pane();
    let mode = h.state().session().study_mode().run(pane).map(|r| r.mode);
    assert_eq!(mode, Some(Mode::Reactive), "Reactive mode is on");
    (h, pane, (words, packs))
}

fn recall(h: &H, pane: PaneId) -> Option<&Recalling> {
    h.state().session().study_mode().recalling(pane)
}

fn phase(h: &H, pane: PaneId) -> Option<Phase> {
    recall(h, pane).map(|r| r.recall.phase)
}

/// The outcomes of the words `pane` recalled this run.
fn recalled(h: &H, pane: PaneId) -> Vec<Outcome> {
    let run = h.state().session().study_mode().run(pane);
    run.map(|r| r.recalled.iter().map(|(.., o)| *o).collect())
        .unwrap_or_default()
}

/// Steps until `done` holds; frames keep coming while the AI answers.
fn wait(h: &mut H, done: impl Fn(&H) -> bool) -> bool {
    for _ in 0..300 {
        if done(h) {
            return true;
        }
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(5));
    }
    done(h)
}

/// Types `text` in the answer box and checks it.
fn answer(h: &mut H, text: &str) {
    h.get_by_role(Role::MultilineTextInput).type_text(text);
    h.run_steps(2);
    h.get_by_label(CHECK).click();
    h.run_steps(2);
}

#[test]
fn a_shaded_word_is_recalled_wrong_then_right_and_memorized() {
    let (mut h, pane, _shelves) = recalling("ready");
    click(&mut h, "quick");
    assert_eq!(
        recall(&h, pane).map(|r| r.word.form.as_str()),
        Some("quick")
    );
    assert_eq!(h.state().study().card_word(), None, "Explain stays shut");
    answer(&mut h, "slow");
    assert!(
        wait(&mut h, |h| phase(h, pane) == Some(Phase::Result)),
        "judged"
    );
    h.run_steps(2);
    assert!(h.query_by_label("Not quite").is_some());
    assert!(
        h.query_by_label_contains("a synonym starts with").is_some(),
        "a hint"
    );
    h.get_by_label(AGAIN).click();
    h.run_steps(2);
    answer(&mut h, "right");
    assert!(
        wait(&mut h, |h| phase(h, pane) == Some(Phase::Ask)),
        "correct"
    );
    h.run_steps(2);
    assert!(h.query_by_label_contains("to memorized?").is_some());
    assert_eq!(recall(&h, pane).map(|r| r.recall.attempts), Some(2));
    h.get_by_label(MEMORIZED).click();
    h.run_steps(3);
    assert!(recall(&h, pane).is_none(), "the recall ends");
}

#[test]
fn a_failing_judge_keeps_the_answer_and_counts_no_try() {
    let (mut h, pane, _shelves) = recalling("failing");
    h.state_mut().set_option("recall", "quick:right").unwrap();
    let failed = |h: &H| phase(h, pane) == Some(Phase::Failed);
    assert!(wait(&mut h, failed), "the judge fails");
    h.run_steps(2);
    assert!(h.query_by_label_contains("Nothing was counted").is_some());
    let kept = recall(&h, pane).map(|r| (r.answer.as_str(), r.recall.attempts));
    assert_eq!(kept, Some(("right", 0)));
    h.get_by_label(CANCEL).click();
    h.run_steps(3);
    assert!(recall(&h, pane).is_none(), "cancel closes it");
}

#[test]
fn i_don_t_know_shows_the_meaning_and_saves_it_as_missed() {
    let (mut h, pane, _shelves) = recalling("ready");
    click(&mut h, "quick");
    assert!(recall(&h, pane).is_some(), "the recall opens");
    h.get_by_label(UNSURE).click();
    h.run_steps(3);
    assert_eq!(phase(&h, pane), Some(Phase::Revealed));
    assert!(h.query_by_label_contains("Saved as missed").is_some());
    assert!(h.query_by_label("sample").is_some(), "the synonym shows");
    h.get_by_label(GOT_IT).click();
    h.run_steps(3);
    assert!(recall(&h, pane).is_none());
    assert_eq!(recalled(&h, pane), [Outcome::Missed]);
    // A settled word takes the missed look and opens no recall again.
    click(&mut h, "quick");
    assert!(recall(&h, pane).is_none(), "settled");
}

#[test]
fn with_asking_off_a_right_answer_is_memorized_at_once() {
    let (mut h, pane, _shelves) = recalling("ready");
    h.state_mut().queue(Sent::Engine(Command::SetField {
        path: "study.mode.ask_memorize".into(),
        value: "false".into(),
    }));
    h.run_steps(2);
    assert!(!h.state().settings().study.mode.ask_memorize);
    h.state_mut().set_option("recall", "quick:right").unwrap();
    let saved = |h: &H| recalled(h, pane) == [Outcome::Memorized];
    assert!(wait(&mut h, saved), "memorized without asking");
    assert!(recall(&h, pane).is_none());
}

#[test]
fn the_study_mode_tab_shows_each_group() {
    let (mut h, _pane, _shelves) = recalling("ready");
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Study)));
    h.run_steps(3);
    assert_eq!(h.state().settings_window().tab(), Tab::Study);
    for label in ["Batches", "Proactive", "Reactive", "Prepare ahead"] {
        let shown = h.query_all_by_label_contains(label).count();
        assert!(shown > 0, "{label} missing");
    }
    let ask = h.query_all_by_label_contains("Ask before adding").count();
    assert!(ask > 0, "the Reactive switch");
}

/// PROGRESS.md row 9: a judge at work while its word is away has a line.
#[test]
fn a_judge_out_of_view_works_in_the_ai_stack() {
    let (mut h, pane, _shelves) = recalling("silent");
    h.state_mut().set_option("recall", "quick:right").unwrap();
    h.run_steps(10);
    let judging = recall(&h, pane).and_then(Recalling::judging);
    assert_eq!(judging, Some(false), "the judge works on it");
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page: 4 }));
    h.run_steps(20);
    assert!(recall(&h, pane).is_some(), "the recall waits");
    h.get_by_label(STACK).hover();
    h.run_steps(20);
    h.get_by_label_contains("Checking your answer · “quick”");
}
