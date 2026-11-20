//! Study-mode gate, plan and memorized-word behaviour.

use std::collections::{HashMap, HashSet};

use mareader_study::{BatchUnit, Candidate, Mode, build_batches};
use mareader_workspace::PaneId;

use super::{State, cfg_of, deck_for};

fn sizes(n: usize, each: usize) -> Vec<usize> {
    vec![each; n]
}

fn state_with_pages(n: usize) -> State {
    let mut s = State::default();
    s.plan(sizes(n, 400), &cfg_of(BatchUnit::Pages, 3, 0));
    s
}

#[test]
fn plan_builds_batches_and_rounds_unlock_to_a_batch_end() {
    let mut s = state_with_pages(7);
    assert_eq!(s.batches().len(), 2);
    s.set_unlocked(1, 4);
    // Re-plan with the same sizes keeps the unlock rounded up.
    s.plan(sizes(7, 400), &cfg_of(BatchUnit::Pages, 3, 0));
    assert_eq!(s.unlocked_to(1), 7);
}

#[test]
fn blocks_past_the_unlock_only_in_proactive() {
    let mut s = state_with_pages(7);
    s.set_mode(Mode::Proactive);
    s.set_unlocked(1, 3);
    assert!(s.blocks(1, 3));
    assert!(!s.blocks(1, 2));
    s.set_mode(Mode::Reactive);
    assert!(!s.blocks(1, 3));
}

#[test]
fn open_gate_unlocks_the_batch_and_marks_known_shrinks_the_timer() {
    let mut s = state_with_pages(7);
    s.set_mode(Mode::Proactive);
    let deck = deck_for(
        vec![
            Candidate {
                lemma: "fortune".into(),
                pos: "any".into(),
                band: 4,
                sentence: "She made a fortune.".into(),
            },
            Candidate {
                lemma: "merely".into(),
                pos: "any".into(),
                band: 5,
                sentence: "It was merely a suggestion.".into(),
            },
        ],
        &HashSet::new(),
        12,
    );
    let gate = s
        .open_gate(PaneId(1), 1, deck, 6, 1_000)
        .expect("gate opens");
    assert_eq!(gate.range(), "pages 1–3");
    assert_eq!(gate.left_secs, 12);
    let left_before = s.gate_left();
    s.mark_known("fortune|any");
    assert!(s.is_memorized("fortune|any"));
    assert!(s.gate_left() < left_before);
    assert_eq!(s.deck_len(), 1);
    let unlocked = s.unlock_gate(1).expect("unlock");
    assert_eq!(unlocked.batch.to + 1, 3);
    assert_eq!(s.unlocked_to(1), 3);
    assert!(s.gate().is_none());
}

#[test]
fn empty_deck_still_unlocks_when_started() {
    let mut s = state_with_pages(7);
    s.set_mode(Mode::Proactive);
    s.open_gate(PaneId(1), 1, deck_for(vec![], &HashSet::new(), 12), 6, 0);
    assert!(s.may_start(true));
    s.unlock_gate(1);
    assert_eq!(s.unlocked_to(1), 3);
}

#[test]
fn candidates_drop_memorized_and_keep_hard_words() {
    let mut bands = HashMap::new();
    bands.insert("fortune".to_owned(), 4u8);
    bands.insert("the".to_owned(), 1u8);
    let text = "She made a fortune. The luck was hers.";
    let known = HashSet::from([mareader_study::word_key("fortune", "any")]);
    let found = super::candidates_in(&[(0, text.to_owned())], &bands, 3, &known);
    assert!(found.is_empty());
    let open = super::candidates_in(&[(0, text.to_owned())], &bands, 3, &HashSet::new());
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].lemma, "fortune");
    assert!(open[0].sentence.contains("fortune"));
}

#[test]
fn reflow_replans_and_keeps_unlocked_text() {
    let mut s = state_with_pages(7);
    s.set_mode(Mode::Proactive);
    s.set_unlocked(1, 3);
    // Smaller pages: more pages, unlock rounds to the batch that held it.
    s.plan(sizes(20, 400), &cfg_of(BatchUnit::Pages, 3, 0));
    assert!(s.unlocked_to(1) >= 3);
    assert_eq!(s.batches()[0].page_count, 3);
}

#[test]
fn memorized_is_global_and_reversible() {
    let mut s = State::default();
    assert!(s.mark_known("visit|verb"));
    assert!(s.is_memorized("visit|verb"));
    assert!(s.forget("visit|verb"));
    assert!(!s.is_memorized("visit|verb"));
}

#[test]
fn batch_helper_matches_build_batches() {
    let cfg = cfg_of(BatchUnit::Pages, 3, 0);
    let direct = build_batches(&sizes(6, 400), &cfg);
    let mut s = State::default();
    s.plan(sizes(6, 400), &cfg);
    assert_eq!(s.batches(), direct.as_slice());
}
