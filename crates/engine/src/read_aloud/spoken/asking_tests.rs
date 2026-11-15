//! A table asked about, in the engine: the question, and both its answers.
//!
//! The book is the epub sample's first chapter, which holds a table part way
//! in, so a reading starts at its beginning and frames until the reader holds
//! at that table. The feed below is a thread, and when a sentence lands is not
//! the test's to say.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use mareader_session::{Budget, Event, Session, Source};
use mareader_settings::Skip;
use mareader_speech::SILENT;

use super::*;
use crate::read_aloud::parts::Kind;
use crate::read_aloud::place::Panes;
use crate::read_aloud::player::Act;

/// Frames a test waits through, and the pause between them.
const FRAMES: usize = 400;
const WAIT: Duration = Duration::from_millis(5);

/// The pane a test's book stands in.
const PANE: PaneId = PaneId(7);

/// A hint for the seek track: no page is named here.
fn hint(_: f32) -> Option<String> {
    None
}

/// Panes that keep nothing: what a table asks is all these tests watch.
struct Glass;

impl Panes for Glass {
    fn go_to(&mut self, _: PaneId, _: usize) {}

    fn reveal_said(&mut self, _: PaneId, _: usize, _: Range<usize>, _: f32) -> bool {
        true
    }
}

/// One reading under test: the reader, its book and what it reads with.
struct Read {
    spoken: Spoken,
    glass: Glass,
    stand: Stand,
    settings: ReadAloudSettings,
    shelf: Shelf,
}

impl Read {
    /// Frames until `met` holds; false if `FRAMES` of them did not.
    fn frames(&mut self, met: impl Fn(&Self) -> bool) -> bool {
        for _ in 0..FRAMES {
            if met(self) {
                return true;
            }
            let (settings, shelf, stand) = (&self.settings, &self.shelf, &self.stand);
            self.spoken
                .tick(&mut self.glass, Some(stand), (settings, shelf, 0.0));
            std::thread::sleep(WAIT);
        }
        met(self)
    }

    /// The part reading is in and can leave, as the player shows it.
    fn in_part(&self) -> Option<Kind> {
        self.spoken.view(&hint).escape
    }

    /// True once reading has lit a sentence other than `was`.
    fn moved(&self, was: &(usize, Range<usize>)) -> bool {
        self.spoken.spot().is_some_and(|now| &now != was)
    }
}

/// The sample's first chapter opened here, standing in a pane of its own.
fn stood() -> Option<Stand> {
    let mut session = Session::new(0, || {});
    let budget = Budget {
        items: 4,
        bytes: usize::MAX,
    };
    let bytes = Arc::new(mareader_epub::sample::epub(1, "Sample"));
    session.open(Source::Bytes(bytes), None, Default::default());
    let mut found = None;
    for _ in 0..FRAMES {
        let id = session.poll(budget).into_iter().find_map(|e| match e {
            Event::Opened { result: Ok(id), .. } => Some(id),
            _ => None,
        });
        found = id;
        if id.is_some() {
            break;
        }
        std::thread::sleep(WAIT);
    }
    let doc = found?;
    let document = Arc::clone(&session.get(doc)?.doc);
    Some(Stand {
        pane: PANE,
        doc,
        document,
        current: 0,
    })
}

/// A reading of the sample, with `tables` deciding what its table does.
fn reading(tables: Skip) -> Result<Read, &'static str> {
    let stand = stood().ok_or("the sample didn't open")?;
    let repaint = Repaint::new(Arc::new(|_: Duration| {}));
    let settings = ReadAloudSettings {
        engine: SILENT.to_owned(),
        skips: Skips {
            tables,
            ..Default::default()
        },
        ..Default::default()
    };
    let shelf = Shelf::default();
    let start = Start::Spot(Spot::default());
    let spoken = Spoken::new(repaint, &stand, (start, None), (&settings, &shelf));
    Ok(Read {
        spoken,
        glass: Glass,
        stand,
        settings,
        shelf,
    })
}

/// A table that is no plain prose, and is asked about each time.
fn asked() -> Skip {
    Skip {
        on: true,
        ask: true,
    }
}

#[test]
fn a_table_holds_reading_to_ask_and_is_read_on_yes() -> Result<(), &'static str> {
    let mut read = reading(asked())?;
    let table = Some(Kind::Table);

    assert!(read.frames(|r| r.spoken.is_asking()), "the table asks");
    let view = read.spoken.view(&hint);
    assert_eq!(view.ask, table, "named as a table");
    assert!(view.escape.is_none(), "nothing to leave while asking");
    let held = read.spoken.spot().ok_or("no spot while asking")?;
    let cost = |r: &Read| r.spoken.ask_cost(1.0).is_some();
    assert!(read.frames(cost), "and what reading it costs");

    read.spoken.act(Act::Answer(true));
    assert!(!read.spoken.is_asking(), "the answer is taken");
    let cells = |r: &Read| r.in_part() == table && r.moved(&held);
    assert!(read.frames(cells), "a later cell is read on");
    assert!(read.spoken.ask_cost(1.0).is_none(), "no question is left");
    Ok(())
}

#[test]
fn a_table_holds_reading_to_ask_and_is_passed_on_no() -> Result<(), &'static str> {
    let mut read = reading(asked())?;
    let table = Some(Kind::Table);

    assert!(read.frames(|r| r.spoken.is_asking()), "the table asks");
    assert_eq!(read.in_part(), None, "nothing to leave while asking");
    let held = read.spoken.spot().ok_or("no spot while asking")?;

    read.spoken.act(Act::Answer(false));
    assert!(!read.spoken.is_asking(), "the answer is taken");
    let past = |r: &Read| r.in_part() != table && r.moved(&held);
    assert!(read.frames(past), "reading went on past the table");
    assert!(read.spoken.lit().is_some(), "and words are lit past it");
    Ok(())
}
