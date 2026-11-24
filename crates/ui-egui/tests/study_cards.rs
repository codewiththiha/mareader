//! Study mode's AI cards, headless: the waiting card and its slow offer,
//! the error card's way to the dictionary or to the setup it lacks, decks
//! made ahead, and the corner stacks while the AI writes out of view.

use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::settings::Tab;
use mareader_engine::study_mode::{Ahead, DeckState, Setup};
use mareader_study::mode::Via;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::activity::stack::{CANCEL, STACK};
use mareader_ui_egui::study_mode::confirm::TURN_OFF;
use mareader_ui_egui::study_mode::deck::START;
use mareader_ui_egui::study_mode::flip_card::TURN_HINT;
use mareader_ui_egui::study_mode::waiting::{CHECK_MODEL, DICTIONARY, RETRY, SLOW, stage};
use mareader_ui_egui::ui_command::Sent;
use mareader_workspace::{PaneId, SplitAxis};

use crate::common::{Dials, levels_reader, shelves};

type H = Harness<'static, MareaderApp>;
type Shelves = (tempfile::TempDir, tempfile::TempDir);

/// A reader studying a page a batch, a scripted AI writing the cards.
fn studying(cards: &str, ahead: u8) -> (H, PaneId, Shelves) {
    studying_at(cards, ahead, None)
}

/// The same, turned on at `page`, the pages before it free to read.
fn studying_at(cards: &str, ahead: u8, page: Option<usize>) -> (H, PaneId, Shelves) {
    let (words, packs) = shelves();
    let mut h = levels_reader(words.path(), Some(packs.path()));
    h.dial(|s| {
        s.study.mode.limit = false;
        s.study.mode.ahead = ahead;
        s.study.mode.pages = 1;
    });
    h.state_mut().set_option("study_cards", cards).unwrap();
    if let Some(page) = page {
        go_to(&mut h, page);
    }
    h.state_mut().set_option("study_mode", "proactive").unwrap();
    h.run_steps(4);
    let pane = h.state().active_pane();
    (h, pane, (words, packs))
}

fn go_to(h: &mut H, page: usize) {
    let pane = h.state().active_pane();
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page }));
    h.run_steps(30);
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
    assert!(h.query_by_label_contains("couldn't be reached").is_some());
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
fn a_deck_turned_away_for_its_key_leads_with_the_setup() {
    let (mut h, pane, _shelves) = studying("keyless", 1);
    let setup = |h: &H| match deck(h, pane) {
        Some(DeckState::Failed { setup, .. }) => *setup,
        _ => None,
    };
    let failed = wait(&mut h, |h| setup(h) == Some(Setup::Model));
    assert!(failed, "the model wants a key");
    h.run_steps(2);
    assert!(
        h.query_by_label(RETRY).is_some(),
        "a retry once the key is in"
    );
    assert!(h.query_by_label(DICTIONARY).is_some(), "the pack still can");
    h.get_by_label(CHECK_MODEL).click();
    h.run();
    assert_eq!(h.state().settings_window().tab(), Tab::Models);
}

#[test]
fn decks_made_ahead_open_at_once() {
    let (mut h, pane, _shelves) = studying("ready", 2);
    assert!(
        wait(&mut h, |h| open_via(h, pane) == Some(Via::Ai)),
        "the AI writes"
    );
    h.run_steps(2);
    h.get_by_label_contains(TURN_HINT).click();
    h.run_steps(30);
    assert!(
        h.query_by_label_contains("Like: sample").is_some(),
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
}

#[test]
fn the_stack_shows_the_writing_only_while_the_deck_is_out_of_view() {
    let (mut h, pane, _shelves) = studying_at("silent", 1, Some(3));
    assert!(wait(&mut h, |h| writing(h, pane)), "the AI is asked");
    assert_eq!(h.state().session().study_gate(pane), Some(3));
    h.run_steps(4);
    // The waiting card says it.
    crate::common::assert_seen(&h);
    go_to(&mut h, 0);
    let stacked = |h: &H| h.query_by_label(STACK).is_some();
    assert!(
        wait(&mut h, stacked),
        "away from the gate, the corner says it"
    );
    assert!(
        h.query_by_label_contains("· page 4").is_none(),
        "folded, it says nothing"
    );
    h.get_by_label(STACK).hover();
    h.run_steps(20);
    assert!(
        h.query_by_label_contains("· page 4").is_some(),
        "a hover fans it out to say so"
    );
    h.get_by_label(CANCEL).click();
    h.run_steps(4);
    h.get_by_label(TURN_OFF).click();
    h.run_steps(4);
    assert!(
        !h.state().session().study_mode().on(pane),
        "study mode is off"
    );
    let toast = h.state().toast().map(|t| t.message.clone());
    let said = toast.unwrap_or_default();
    assert!(
        said.starts_with("Stopped writing"),
        "a toast says so: {said:?}"
    );
    go_to(&mut h, 3);
    assert!(
        h.query_by_label(STACK).is_none(),
        "nothing is left in the corner"
    );
}

/// PROGRESS.md §5, steps 1, 3 and 4: the library's run stacks once until a
/// pane's job needs the corner, then joins every pane's stack.
#[test]
fn the_app_s_work_joins_every_pane_once_a_pane_needs_its_corner() {
    let (mut h, pane, _shelves) = studying_at("silent", 1, Some(3));
    h.state_mut().set_option("copies", "5").unwrap();
    h.state_mut().set_option("organize", "running").unwrap();
    assert!(wait(&mut h, |h| writing(h, pane)), "the AI is asked");
    let stacks = |h: &H| h.query_all_by_label(STACK).count();
    assert!(wait(&mut h, |h| stacks(h) == 1), "organizing stacks once");
    let split = Sent::Engine(Command::Split(SplitAxis::Horizontal));
    h.state_mut().queue(split);
    h.run_steps(30);
    assert_eq!(stacks(&h), 1, "two panes: still one, the deck says its own");
    let away = Sent::Engine(Command::GoTo { pane, page: 0 });
    h.state_mut().queue(away);
    assert!(
        wait(&mut h, |h| stacks(h) == 2),
        "each pane stacks organizing"
    );
    let back = Sent::Engine(Command::GoTo { pane, page: 3 });
    h.state_mut().queue(back);
    assert!(wait(&mut h, |h| stacks(h) == 1), "back to the one stack");
}
