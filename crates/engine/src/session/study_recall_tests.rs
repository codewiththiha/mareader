//! Keeping a recall's answer text and written back in a book.

use std::path::PathBuf;

use mareader_library::{ContentId, DeviceId, Format, Measure, Opened, RecallAnswer};
use mareader_study::mode::Written;

use super::*;

#[test]
fn keeping_and_dismissing_sessions_preserves_answers_and_existing_status() {
    let mut lib = Library::new(DeviceId(1));
    let book = lib.record_open(
        Opened {
            path: PathBuf::from("/recall.epub"),
            measure: Measure {
                content: ContentId(1),
                stamp: Default::default(),
            },
            format: Format::Epub,
            title: "A book".into(),
            author: None,
        },
        1_000,
    );
    let mut recall = Recalling::new(
        PaneId(1),
        Word {
            key: Key::new("quick", None),
            form: "quick".into(),
            sentence: "A quick walk.".into(),
            band: 4,
            page: 0,
            chars: 2..7,
            spot: Some(mareader_library::Spot::Blocks {
                from: [1, 2],
                to: [1, 7],
            }),
        },
        None,
        String::new(),
    );
    recall.back = Some(Written {
        meaning: "fast".into(),
        synonyms: vec!["brisk".into()],
        usage: "A quick reply.".into(),
        ..Written::default()
    });
    recall.answers.push(RecallAnswer {
        text: "fast".into(),
        verdict: "Correct".into(),
        feedback: "Yes".into(),
    });
    let done = Done {
        outcome: Outcome::Memorized,
        attempts: 1,
        first_try: true,
    };
    keep(&mut lib, book, &recall, done, false);
    recall.answers[0].text = "rapid".into();
    let dismissed = Done {
        outcome: Outcome::Learning,
        attempts: 1,
        first_try: false,
    };
    keep(&mut lib, book, &recall, dismissed, true);
    let seen = lib.sightings(book).into_iter().next().unwrap();
    assert_eq!(seen.recall_history.sessions.len(), 2);
    assert_eq!(
        seen.location(),
        Some(mareader_library::Spot::Blocks {
            from: [1, 2],
            to: [1, 7]
        })
    );
    assert_eq!(seen.synonyms.value, ["brisk"]);
    assert_eq!(seen.usages.value, ["A quick reply."]);
    assert_eq!(
        lib.word(seen.word).unwrap().status.value,
        WordStatus::Memorized
    );
}
