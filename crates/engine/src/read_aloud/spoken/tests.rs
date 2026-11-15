//! A reading driven with no window: a recorded `Repaint`, panes that keep
//! what they were asked, and a book opened in memory.
//!
//! The feed below is a thread, so a test frames until what it waits for is
//! there, and `frames` bounds that wait rather than trusting a count.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mareader_pdf::sample::pdf;
use mareader_session::{Budget, Event, Session, Source};
use mareader_speech::SILENT;

use super::*;
use crate::read_aloud::TICK;
use crate::read_aloud::lit::Span;
use crate::read_aloud::place::Panes;
use crate::read_aloud::player::{Act, View};

/// Frames a test waits through, and the pause between them.
const FRAMES: usize = 400;
const WAIT: Duration = Duration::from_millis(5);

/// The pane a test's book stands in, and one it can be rehomed to.
const PANE: PaneId = PaneId(7);
const MOVED: PaneId = PaneId(9);

/// A hint for the seek track: a share past the end has no page to name.
fn none(share: f32) -> Option<String> {
    (share > 1.0).then(|| "past the end".to_owned())
}

/// The in-memory samples opened here, in one session so their ids differ.
fn stands(of: &[(PaneId, usize)]) -> Option<Vec<Stand>> {
    let mut session = Session::new(0, || {});
    let budget = Budget {
        items: 1,
        bytes: usize::MAX,
    };
    let mut books = Vec::new();
    for &(pane, pages) in of {
        let bytes = Arc::new(pdf(pages, "Sample"));
        session.open(Source::Bytes(bytes), None, Default::default());
        let id = session.poll(budget).into_iter().find_map(|e| match e {
            Event::Opened { result: Ok(id), .. } => Some(id),
            _ => None,
        })?;
        let document = Arc::clone(&session.get(id)?.doc);
        books.push(Stand {
            pane,
            doc: id,
            document,
            current: 0,
        });
    }
    Some(books)
}

/// A window recording the waits asked of it, in order.
fn recording() -> (Repaint, Arc<Mutex<Vec<Duration>>>) {
    let asks: Arc<Mutex<Vec<Duration>>> = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&asks);
    let repaint = Repaint::new(Arc::new(move |after| {
        if let Ok(mut log) = kept.lock() {
            log.push(after);
        }
    }));
    (repaint, asks)
}

/// The waits asked for; a poisoned lock reads as none asked.
fn asked(asks: &Mutex<Vec<Duration>>) -> Vec<Duration> {
    asks.lock().map_or_else(|_| Vec::new(), |log| log.clone())
}

/// The panes a reader turns and lights, keeping what was asked of it.
struct Glass {
    turned: Vec<(PaneId, usize)>,
    lit: Vec<(PaneId, usize, Range<usize>)>,
    /// False as a pane that has stopped moving, so nothing lit is kept.
    moves: bool,
}

impl Default for Glass {
    fn default() -> Self {
        Self {
            turned: Vec::new(),
            lit: Vec::new(),
            moves: true,
        }
    }
}

impl Panes for Glass {
    fn go_to(&mut self, pane: PaneId, page: usize) {
        self.turned.push((pane, page));
    }

    fn reveal_said(&mut self, pane: PaneId, page: usize, chars: Range<usize>, _: f32) -> bool {
        self.lit.push((pane, page, chars));
        self.moves
    }
}

/// One reading under test: the reader, its panes, its book, its waits.
struct Read {
    spoken: Spoken,
    glass: Glass,
    stand: Stand,
    /// Books opened beside it, to be taken on or refused.
    others: Vec<Stand>,
    settings: ReadAloudSettings,
    shelf: Shelf,
    asks: Arc<Mutex<Vec<Duration>>>,
}

impl Read {
    /// One frame, with its book standing in a pane.
    fn frame(&mut self) -> bool {
        let (settings, shelf, stand) = (&self.settings, &self.shelf, &self.stand);
        self.spoken
            .tick(&mut self.glass, Some(stand), (settings, shelf, 0.0))
    }

    /// One frame, with its book away from any pane.
    fn away(&mut self) -> bool {
        let (settings, shelf) = (&self.settings, &self.shelf);
        self.spoken
            .tick(&mut self.glass, None, (settings, shelf, 0.0))
    }

    /// Frames until `met` holds; false if `FRAMES` of them did not.
    fn frames(&mut self, met: impl Fn(&Self) -> bool) -> bool {
        for _ in 0..FRAMES {
            if met(self) {
                return true;
            }
            self.frame();
            std::thread::sleep(WAIT);
        }
        met(self)
    }

    /// What the player would show of it.
    fn view(&self) -> View<'static> {
        self.spoken.view(&none)
    }
}

/// A reading of `pages` of the sample, with `others` opened beside it.
fn reading(
    pages: usize,
    others: &[(PaneId, usize)],
    until: Option<Spot>,
) -> Result<Read, &'static str> {
    let mut wants = vec![(PANE, pages)];
    wants.extend_from_slice(others);
    let mut books = stands(&wants).ok_or("the sample didn't open")?;
    let stand = books.remove(0);
    let (repaint, asks) = recording();
    let settings = ReadAloudSettings {
        engine: SILENT.to_owned(),
        ..Default::default()
    };
    let shelf = Shelf::default();
    let start = Start::Spot(Spot::default());
    let spoken = Spoken::new(repaint, &stand, (start, until), (&settings, &shelf));
    Ok(Read {
        spoken,
        glass: Glass::default(),
        stand,
        others: books,
        settings,
        shelf,
        asks,
    })
}

/// A reading of the whole sample, with no other book beside it.
fn mute(pages: usize) -> Result<Read, &'static str> {
    reading(pages, &[], None)
}

#[test]
fn a_reading_names_its_engine_and_the_book_it_reads() -> Result<(), &'static str> {
    let mut read = mute(1)?;
    assert_eq!(read.spoken.engine(), SILENT);
    assert!(read.spoken.reads(&read.stand), "its own book");
    assert!(read.spoken.playing(), "a mute engine still opens");
    assert!(!read.spoken.is_asking(), "plain prose asks nothing");
    assert!(read.spoken.ask_cost(1.0).is_none());
    assert!(read.frame(), "a frame reads on");
    Ok(())
}

#[test]
fn a_book_opened_again_in_alike_pages_is_taken_on() -> Result<(), &'static str> {
    let mut read = reading(1, &[(PANE, 1)], None)?;
    let again = read.others.first().unwrap().clone();
    assert!(
        !read.spoken.ours(&again),
        "another copy is not the same book"
    );
    assert!(read.spoken.rehome(&again), "pages laid alike read on");
    assert!(read.spoken.reads(&again), "the book taken on is read");
    read.stand = again;
    assert!(read.frame(), "and the reading goes on in it");
    Ok(())
}

#[test]
fn a_book_that_differs_is_not_taken_on() -> Result<(), &'static str> {
    let mut read = reading(1, &[(PANE, 3)], None)?;
    let differs = read.others.first().unwrap().clone();
    assert!(!read.spoken.rehome(&differs), "three pages are not one");
    assert!(read.spoken.reads(&read.stand), "its own book is read");
    assert!(read.frame(), "and it goes on reading it");
    Ok(())
}

#[test]
fn the_first_sentence_read_is_lit_and_named_by_its_page() -> Result<(), &'static str> {
    let mut read = mute(1)?;
    let lit = read.frames(|r| r.spoken.lit().is_some());
    assert!(lit, "the sample is read");
    let said = read.spoken.lit().unwrap();
    assert!(matches!(said.span, Span::Chars { .. }), "in characters");
    let spot = read.spoken.spot().unwrap();
    assert_eq!(spot.0, 0, "the page it stands on");
    assert!(spot.1.start < spot.1.end, "characters are lit");
    let named = read.spoken.whereabouts().unwrap();
    assert!(named.starts_with("Page 1"), "named by page: {named}");
    let lyrics = read.spoken.lyrics().unwrap();
    assert!(!lyrics.now.is_empty(), "the sentence said");
    assert!(lyrics.before.is_none(), "the first has none before");
    assert!(!lyrics.words.is_empty(), "its words are known");
    Ok(())
}

#[test]
fn the_panes_are_asked_to_show_the_sentence_read() -> Result<(), &'static str> {
    let mut read = mute(1)?;
    let lit = read.frames(|r| !r.glass.lit.is_empty());
    assert!(lit, "the pane is asked to show a sentence");
    let (pane, page, chars) = read.glass.lit.first().unwrap().clone();
    assert_eq!(pane, PANE, "the pane the book stands in");
    assert_eq!(page, 0, "its first page");
    assert!(chars.start < chars.end, "the characters read");
    Ok(())
}

#[test]
fn a_line_brought_into_view_is_not_asked_for_again() -> Result<(), &'static str> {
    let mut read = mute(1)?;
    let twice = read.frames(|r| r.glass.lit.len() > 1);
    assert!(twice, "a second sentence is read on to");
    assert_eq!(read.glass.lit.len(), 2, "one ask for each sentence");
    let first = read.glass.lit.first().unwrap().2.clone();
    let last = read.glass.lit.last().unwrap().2.clone();
    assert_ne!(first, last, "each sentence lights its own");
    Ok(())
}

#[test]
fn a_pane_that_stopped_moving_is_asked_every_frame() -> Result<(), &'static str> {
    let mut read = mute(1)?;
    read.glass.moves = false;
    let thrice = read.frames(|r| r.glass.lit.len() > 2);
    assert!(thrice, "a line not brought up is asked again");
    let first = read.glass.lit.first().unwrap().2.clone();
    let second = read.glass.lit.get(1).unwrap().2.clone();
    assert_eq!(first, second, "the same line, asked twice");
    Ok(())
}

#[test]
fn away_from_its_pane_a_book_reads_on_unseen() -> Result<(), &'static str> {
    let mut read = mute(1)?;
    let lit = read.frames(|r| r.spoken.lit().is_some());
    assert!(lit, "the sample is read");
    read.glass.lit.clear();
    read.glass.turned.clear();
    assert!(read.away(), "a frame away from the pane reads on");
    assert!(read.spoken.playing(), "the book is still reading");
    assert!(read.glass.lit.is_empty(), "nothing is shown in a pane");
    assert!(read.glass.turned.is_empty(), "no pane is turned");
    assert!(asked(&read.asks).contains(&TICK), "a frame is asked");
    let on = read.frames(|r| r.spoken.lit().is_some());
    assert!(on, "and it reads on unseen");
    Ok(())
}

#[test]
fn a_pause_holds_the_line_and_a_play_reads_on() -> Result<(), &'static str> {
    let mut read = mute(1)?;
    let one = read.frames(|r| r.glass.lit.len() == 1);
    assert!(one, "the first sentence is lit");
    read.spoken.pause();
    assert!(!read.spoken.playing(), "paused, it is not playing");
    for _ in 0..5 {
        assert!(read.frame(), "a paused reading still frames");
    }
    assert_eq!(read.glass.lit.len(), 1, "and holds its line in view");
    read.spoken.toggle();
    assert!(read.spoken.playing(), "played, it reads on");
    let two = read.frames(|r| r.glass.lit.len() > 1);
    assert!(two, "the next sentence is lit in turn");
    Ok(())
}

#[test]
fn a_step_back_says_the_sentence_before_again() -> Result<(), &'static str> {
    let mut read = mute(1)?;
    let kept = |r: &Read| r.spoken.lyrics().and_then(|l| l.before).is_some();
    let back = read.frames(kept);
    assert!(back, "a sentence is kept to step back to");
    let before = read.spoken.lyrics().and_then(|l| l.before).unwrap();
    assert!(!read.spoken.act(Act::Back), "a step back reads on");
    let now = read.spoken.lyrics().unwrap().now;
    assert_eq!(now, before, "the sentence before is said again");
    assert!(read.spoken.playing(), "and the reading goes on");
    Ok(())
}

#[test]
fn a_seek_turns_the_pane_below_and_reads_on_there() -> Result<(), &'static str> {
    let mut read = mute(6)?;
    let lit = read.frames(|r| r.spoken.lit().is_some());
    assert!(lit, "the sample is read");
    assert!(read.view().seek, "a whole book can be sought");
    let hint = read.spoken.hint(0.5).unwrap();
    assert!(hint.starts_with("Page 4"), "half of six pages: {hint}");
    assert!(!read.spoken.act(Act::Seek(0.5)), "a seek reads on");
    assert!(read.frame(), "and the next frame reads on");
    let there = (PANE, 3);
    assert!(read.glass.turned.contains(&there), "the pane turns");
    Ok(())
}

#[test]
fn a_selection_reads_alone_and_says_so_at_its_end() -> Result<(), &'static str> {
    let until = Spot { page: 0, char: 1 };
    let mut read = reading(1, &[], Some(until))?;
    assert!(!read.view().seek, "a selection cannot be sought");
    assert!(read.spoken.hint(0.5).is_none(), "no page is named");
    let end = read.frames(|r| r.view().note.is_some());
    assert!(end, "a selection ends at its end");
    let note = read.view().note.unwrap();
    assert_eq!(note.0, "Selection read", "what the player says");
    assert!(!note.1, "and that is not a fault");
    assert!(!read.spoken.playing(), "finished, it is not playing");
    read.spoken.toggle();
    assert!(read.spoken.playing(), "played, it reads it again");
    Ok(())
}

#[test]
fn a_book_set_in_new_pages_reads_on_from_where_it_stood() -> Result<(), &'static str> {
    let mut read = reading(1, &[(PANE, 1)], None)?;
    let lit = read.frames(|r| r.spoken.lit().is_some());
    assert!(lit, "the sample is read");
    let laid = read.others.first().unwrap().clone();
    read.stand.document = Arc::clone(&laid.document);
    assert!(read.frame(), "a frame takes the new pages");
    let on = read.frames(|r| r.spoken.lit().is_some());
    assert!(on, "the sentence said is found on them");
    assert!(read.spoken.playing(), "and the reading goes on");
    Ok(())
}

#[test]
fn a_book_rehomed_to_a_new_pane_shows_the_words_read() -> Result<(), &'static str> {
    let mut read = reading(1, &[(MOVED, 1)], None)?;
    let lit = read.frames(|r| r.spoken.lit().is_some());
    assert!(lit, "the sample is read");
    let page = read.spoken.spot().unwrap().0;
    let moved = read.others.first().unwrap().clone();
    assert!(read.spoken.rehome(&moved), "read on in the new pane");
    read.stand = moved;
    read.glass.turned.clear();
    assert!(read.frame(), "a frame in the new pane reads on");
    let there = (MOVED, page);
    assert!(read.glass.turned.contains(&there), "it turns there");
    assert!(read.spoken.reads(&read.stand), "the pane it moved to");
    Ok(())
}

#[test]
fn a_restart_reads_again_from_the_start_asked_for() -> Result<(), &'static str> {
    let mut read = mute(1)?;
    let back = read.frames(|r| r.view().back);
    assert!(back, "a sentence is kept to step back to");
    let from = (Start::Spot(Spot::default()), None);
    let (settings, shelf, stand) = (&read.settings, &read.shelf, &read.stand);
    read.spoken.restart(stand, from, (settings, shelf));
    assert!(read.spoken.playing(), "reading again from the start");
    assert!(!read.view().back, "with nothing kept behind it");
    let lit = read.frames(|r| r.spoken.lit().is_some());
    assert!(lit, "the first sentence is lit again");
    assert_eq!(read.spoken.spot().unwrap().0, 0, "on its first page");
    Ok(())
}

#[test]
fn stopping_is_the_one_act_that_ends_a_reading() -> Result<(), &'static str> {
    let mut read = mute(1)?;
    assert!(!read.spoken.act(Act::Card(true)), "a card is drawn above");
    assert!(!read.spoken.act(Act::Reveal), "a line is brought up");
    assert!(!read.spoken.act(Act::Answer(false)), "nothing is asked");
    assert!(read.spoken.act(Act::Stop), "stopping ends the reading");
    Ok(())
}

#[test]
fn every_frame_read_on_asks_the_window_below_for_one() -> Result<(), &'static str> {
    let mut read = mute(1)?;
    assert!(
        !asked(&read.asks).contains(&TICK),
        "opening asks for no frame"
    );
    read.frame();
    read.frame();
    let asks = asked(&read.asks);
    let ticks = asks.iter().filter(|a| **a == TICK).count();
    assert!(ticks > 1, "each frame asks for the next one");
    assert!(
        asks.iter().all(|a| *a == TICK || a.is_zero()),
        "a tick, or at once"
    );
    Ok(())
}
