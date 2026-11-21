//! Study mode's AI cards, headless: the waiting card and its slow offer,
//! the error card's way to the dictionary, decks made ahead, and the pill
//! before the gate.

use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::study_mode::{Ahead, DeckState};
use mareader_study::mode::Via;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::study_mode::deck::{REVEAL, START};
use mareader_ui_egui::study_mode::pill::OPEN;
use mareader_ui_egui::study_mode::waiting::{DICTIONARY, RETRY, SLOW, stage};
use mareader_workspace::PaneId;

use crate::common::{Dials, levels_reader, shelves};

type H = Harness<'static, MareaderApp>;
type Shelves = (tempfile::TempDir, tempfile::TempDir);

/// A reader studying a page a batch, a scripted AI writing the cards.
fn studying(cards: &str, ahead: u8) -> (H, PaneId, Shelves) {
    let (words, packs) = shelves();
    let mut h = levels_reader(words.path(), Some(packs.path()));
    h.dial(|s| {
        s.study.mode.strict = false;
        s.study.mode.ahead = ahead;
        s.study.mode.pages = 1;
    });
    h.state_mut().set_option("study_cards", cards).unwrap();
    h.state_mut().set_option("study_mode", "proactive").unwrap();
    h.run_steps(4);
    let pane = h.state().active_pane();
    (h, pane, (words, packs))
}

/// Steps until `done` holds; frames keep coming while a deck is made.
fn wait(h: &mut H, done: impl Fn(&H) -> bool) -> bool {
    for _ in 0..600 {
        if done(h) {
            return true;
        }
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(5));
    }
    done(h)
}

fn deck(h: &H, pane: PaneId) -> Option<&DeckState> {
    h.state().session().study_mode().run(pane).map(|r| &r.deck)
}

fn open_via(h: &H, pane: PaneId) -> Option<Via> {
    match deck(h, pane) {
        Some(DeckState::Open(open)) => Some(open.via),
        _ => None,
    }
}

fn writing(h: &H, pane: PaneId) -> bool {
    matches!(deck(h, pane), Some(DeckState::Making { job, .. }) if job.via() == Via::Ai)
}

/// What the waiting card says the deck is doing now.
fn said(h: &H, pane: PaneId) -> String {
    match deck(h, pane) {
        Some(DeckState::Making { batch, job }) => stage(&batch.label(), Some(job)),
        _ => String::new(),
    }
}

#[test]
fn a_slow_ai_offers_the_dictionary_which_then_writes_the_deck() {
    let (mut h, pane, _shelves) = studying("silent", 1);
    assert!(wait(&mut h, |h| writing(h, pane)), "the AI is asked");
    assert!(wait(&mut h, |h| said(h, pane).starts_with("Writing meanings")));
    h.run_steps(2);
    let stage = said(&h, pane);
    assert!(
        h.query_by_label(&stage).is_some(),
        "the card shows {stage:?}"
    );
    assert!(h.query_by_label(DICTIONARY).is_none(), "not slow yet");
    // Past the slow mark, on the frames' own clock.
    h.run_steps(460);
    assert!(h.query_by_label(SLOW).is_some(), "the card says it is slow");
    h.get_by_label(DICTIONARY).click();
    h.run_steps(2);
    let opened = wait(&mut h, |h| open_via(h, pane) == Some(Via::Dictionary));
    assert!(opened, "the dictionary writes the deck instead");
}

#[test]
fn a_failed_deck_tries_again_or_turns_to_the_dictionary() {
    let (mut h, pane, _shelves) = studying("failing", 1);
    let failed = |h: &H| matches!(deck(h, pane), Some(DeckState::Failed { .. }));
    assert!(wait(&mut h, failed), "the AI fails");
    h.run_steps(2);
    assert!(h.query_by_label_contains("couldn’t be reached").is_some());
    h.get_by_label(RETRY).click();
    h.run_steps(2);
    assert!(wait(&mut h, failed), "a retry asks again, and fails again");
    h.run_steps(2);
    h.get_by_label(DICTIONARY).click();
    h.run_steps(2);
    let opened = wait(&mut h, |h| open_via(h, pane) == Some(Via::Dictionary));
    assert!(opened, "the dictionary writes the deck");
}

#[test]
fn decks_made_ahead_open_at_once_and_the_pill_leads_to_them() {
    let (mut h, pane, _shelves) = studying("ready", 2);
    assert!(
        wait(&mut h, |h| open_via(h, pane) == Some(Via::Ai)),
        "the AI writes"
    );
    h.run_steps(2);
    h.get_by_label(REVEAL).click();
    h.run_steps(2);
    assert!(
        h.query_by_label("Like: sample").is_some(),
        "the back has synonyms"
    );
    let batches = h
        .state()
        .session()
        .study_mode()
        .run(pane)
        .map_or(0, |r| r.batches.len());
    if batches < 2 {
        return;
    }
    let ready_ahead = |h: &H| {
        let run = h.state().session().study_mode().run(pane);
        run.is_some_and(|r| matches!(r.ahead.first(), Some(Ahead::Ready { .. })))
    };
    assert!(
        wait(&mut h, ready_ahead),
        "the next batch's deck is made ahead"
    );
    h.get_by_label(START).click();
    h.run_steps(3);
    let gate = h.state().session().study_gate(pane);
    assert!(
        gate.is_some_and(|g| g > 0),
        "the first batch unlocks: {gate:?}"
    );
    assert!(
        open_via(&h, pane).is_some(),
        "the next deck is open at once"
    );
    // A gate already in view shows its deck; one out of reach, the pill.
    if h.query_by_label(START).is_some() {
        return;
    }
    assert!(
        h.query_by_label_contains("next words ready").is_some(),
        "the pill tells"
    );
    h.get_by_label(OPEN).click();
    h.run_steps(3);
    let current = h.state().documents().reading(pane).map(|r| r.current);
    assert_eq!(current, gate, "Open words goes to the gate");
}
