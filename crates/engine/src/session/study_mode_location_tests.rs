//! Per-book locations travel with both newly written and reused card backs.

use mareader_library::{ContentId, DeviceId, Format, MeaningBy, Measure, Opened, Spot};
use mareader_study::mode::{Card, Location, Written};

use super::*;

#[test]
fn reused_card_meaning_keeps_each_books_own_word_location() {
    let mut lib = Library::new(DeviceId(1));
    let mut add = |n| {
        lib.record_open(
            Opened {
                path: format!("/book{n}.epub").into(),
                measure: Measure {
                    content: ContentId(n),
                    stamp: Default::default(),
                },
                format: Format::Epub,
                title: format!("Book {n}"),
                author: None,
            },
            1_000,
        )
    };
    let (a, b) = (add(1), add(2));
    let one = Location::Blocks {
        from: [2, 12],
        to: [2, 17],
    };
    let two = Location::Blocks {
        from: [8, 6],
        to: [8, 11],
    };
    for (book, spot) in [(a, one), (b, two)] {
        let card = Card {
            key: Key::new("quick", Some(Pos::Adjective)),
            form: "quick".into(),
            sentence: "A quick walk.".into(),
            spot: Some(spot),
            band: 5,
            count: 1,
            also: Vec::new(),
        };
        let back = Written {
            meaning: "fast".into(),
            ..Written::default()
        };
        record(
            &mut lib,
            book,
            &Shown::written(card, Some(back)),
            WordStatus::Learning,
        );
    }
    assert_eq!(
        lib.sightings(a)[0].location(),
        Some(Spot::Blocks {
            from: [2, 12],
            to: [2, 17]
        })
    );
    assert_eq!(
        lib.sightings(b)[0].location(),
        Some(Spot::Blocks {
            from: [8, 6],
            to: [8, 11]
        })
    );
    assert_eq!(lib.sightings(b)[0].meaning_by.value, Some(MeaningBy::Ai));
}
