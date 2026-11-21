//! Reactive study mode's recall, headless: a shaded word clicked, answered
//! wrong then right, a judge that fails, and an answer revealed.

use std::time::Duration;

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::study_mode::Recalling;
use mareader_study::mode::{Mode, Phase};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::study_mode::recall::{AGAIN, CANCEL, CHECK, GOT_IT, MEMORIZED, UNSURE};
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
    assert_eq!(recall(&h, pane).map(|r| r.word.form.as_str()), Some("quick"));
    assert_eq!(h.state().study().card_word(), None, "Explain stays shut");
    answer(&mut h, "slow");
    assert!(wait(&mut h, |h| phase(h, pane) == Some(Phase::Result)), "judged");
    h.run_steps(2);
    assert!(h.query_by_label("Not quite").is_some());
    assert!(h.query_by_label_contains("a synonym starts with").is_some(), "a hint");
    h.get_by_label(AGAIN).click();
    h.run_steps(2);
    answer(&mut h, "right");
    assert!(wait(&mut h, |h| phase(h, pane) == Some(Phase::Ask)), "correct");
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
}
