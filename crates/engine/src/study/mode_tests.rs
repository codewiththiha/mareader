use std::sync::Arc;

use mareader_library::{LearnedBy, Spot, WordStatus};
use mareader_study::StudyMode;

use super::{Action, WordRecord};
use crate::command::Command;
use crate::host::Fake;
use crate::session::Session;

fn bare() -> Session {
    Session::new(Arc::new(Fake::default()), Arc::new(|| {}))
}

fn record(lemma: &str, status: WordStatus) -> WordRecord {
    WordRecord {
        lemma: lemma.into(),
        class: "noun".into(),
        band: 5,
        meaning: "m".into(),
        context: "c".into(),
        spot: Spot::Page {
            page: 0,
            rect: [0.0; 4],
        },
        by: LearnedBy::Card,
        status,
        attempts: 0,
        first_try: false,
    }
}

#[test]
fn an_unavailable_mode_stays_off_and_says_why() {
    let mut session = bare();
    let pane = session.active();
    let (proactive, reactive) = session.study_availability();
    assert!(!proactive.ok && !reactive.ok);
    session.execute(
        Command::Study(Action::SetMode(pane, StudyMode::Proactive)),
        0.0,
    );
    assert_eq!(session.study_mode(pane), StudyMode::Off);
    assert!(!session.drain().is_empty());
}

#[test]
fn recorded_words_memorize_unmemorize_and_forget() {
    let mut session = bare();
    let pane = session.active();
    let id = session.study_record_word(pane, record("dignity", WordStatus::Learning));
    assert!(id.is_some());
    session.study_record_word(pane, record("dignity", WordStatus::Memorized));
    assert_eq!(session.study_words().len(), 1);
    assert_eq!(session.study_memorized_keys(), ["dignity|noun"]);
    let id = id.unwrap_or(mareader_library::StudyWordId(0));
    session.study_set_word_status(id, WordStatus::Learning);
    assert!(session.study_memorized_keys().is_empty());
    session.study_forget_words(&[id]);
    assert!(session.study_words().is_empty());
}
