//! Study mode through the one door, on a machine without word levels.
//!
//! The full flow, levels and all, runs in `mareader-cli`'s script 6.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;
use std::time::{Duration, Instant};

use mareader_engine::command::Command;
use mareader_engine::documents::OpenRequest;
use mareader_engine::host::Fake;
use mareader_engine::session::Session;
use mareader_engine::study_mode::{Action, Ask, REACTIVE_ONLY};
use mareader_study::mode::{Missing, Mode};

const PAGES: usize = 6;
const WAIT: Duration = Duration::from_secs(5);

fn opened() -> Session {
    let mut session = Session::headless(Arc::new(Fake::default()));
    let sample = mareader_pdf::sample::pdf(PAGES, "Sample");
    session.execute(Command::Open(OpenRequest::bytes(sample, "Sample.pdf")), 0.0);
    let pane = session.active();
    let start = Instant::now();
    while session.documents().page_count(pane).is_none() && start.elapsed() < WAIT {
        session.pump();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        session.documents().page_count(pane).is_some(),
        "the sample opens"
    );
    session.drain();
    session
}

#[test]
fn without_word_levels_the_mode_says_what_it_needs() {
    let mut session = opened();
    let pane = session.active();
    let toggle = Action::Toggle {
        pane,
        mode: Mode::Proactive,
    };
    let done = session.execute(Command::StudyMode(toggle), 1.0);
    assert_eq!(done.refused.as_deref(), Some(Missing::Levels.reason()));
    assert!(!session.study_mode().on(pane));
    assert_eq!(session.study_gate(pane), None);
}

#[test]
fn while_off_a_jump_goes_where_it_was_sent() {
    let mut session = opened();
    let pane = session.active();
    session.execute(Command::GoTo { pane, page: 4 }, 1.0);
    assert_eq!(session.documents().current(pane), Some(4));
    let done = session.execute(Command::StudyMode(Action::Off(pane)), 1.0);
    assert!(!done.dirty, "nothing was on to turn off");
}

#[test]
fn a_recall_opens_only_in_reactive_mode() {
    let mut session = opened();
    let pane = session.active();
    let ask = Ask::Open {
        page: 0,
        chars: 0..3,
    };
    let done = session.execute(Command::StudyMode(Action::Recall { pane, ask }), 1.0);
    assert_eq!(done.refused.as_deref(), Some(REACTIVE_ONLY));
    assert!(session.study_mode().recalling(pane).is_none());
    let close = Action::Recall {
        pane,
        ask: Ask::Close,
    };
    assert!(
        !session.execute(Command::StudyMode(close), 1.0).dirty,
        "nothing to close"
    );
}
