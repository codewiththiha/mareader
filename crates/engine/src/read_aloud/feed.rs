//! A document read into sentences on a thread of its own.
//!
//! The voice says one sentence at a time, so a book is read ahead of it: the
//! reading runs here and sends what it finds. The caller hands in a wake and
//! gets sentences back.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};

use mareader_session::{Anchor, Document, TextLayer};
use mareader_speech::{Glyph, Script, Sentence, Spot, Waker};

use super::parts::{self, Part};

/// Sentences read ahead of the voice; the thread waits past this.
const AHEAD: usize = 8;
/// Sentences after the current handed to the voice: at least this many,
const LOOK: usize = 3;
/// then on to about twenty seconds of speech, in characters,
const LOOK_CHARS: usize = 300;
/// but never more than this many.
const LOOK_MOST: usize = 8;

/// Characters on each page the thread has read, for time left.
pub type Counts = Arc<Mutex<BTreeMap<usize, usize>>>;

/// Where reading starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Start {
    Anchor(Anchor),
    Spot(Spot),
}

/// A sentence off the pages, and its place in the book, if it has one.
#[derive(Clone, Debug)]
pub struct Said {
    pub sentence: Sentence,
    pub anchor: Option<Anchor>,
    /// The tables, lists, figures and asides it starts in, outermost first.
    pub parts: Vec<Part>,
}

/// Where a thread reads from, and where it stops if not at the end.
#[derive(Clone, Copy, Debug)]
pub struct Reach {
    pub start: Start,
    pub until: Option<Spot>,
}

/// How many of `texts` make enough speech to have ready, if they do.
pub fn enough<'a>(texts: impl Iterator<Item = &'a str>) -> Option<usize> {
    let mut chars = 0;
    for (n, text) in (1..).zip(texts) {
        chars += text.chars().count();
        if n >= LOOK_MOST || (n >= LOOK && chars >= LOOK_CHARS) {
            return Some(n);
        }
    }
    None
}

/// Reads `document` into sentences over `span` on a thread of its own.
pub fn spawn(
    document: Arc<Document>,
    span: Reach,
    skip_folios: bool,
    (wake, counts): (Waker, Counts),
) -> Receiver<Said> {
    let (tx, rx) = std::sync::mpsc::sync_channel(AHEAD);
    let thread = std::thread::Builder::new().name("read-aloud".into());
    // A thread that won't start sends nothing: the reading just ends.
    let _ = thread.spawn(move || read(&document, span, skip_folios, (&tx, &wake, &counts)));
    rx
}

/// Sends `document`'s sentences until the end, or until no one listens.
fn read(
    document: &Document,
    span: Reach,
    skip_folios: bool,
    (tx, wake, counts): (&SyncSender<Said>, &Waker, &Counts),
) {
    let (first, from) = match span.start {
        Start::Spot(spot) => (spot.page, spot.char),
        Start::Anchor(anchor) => locate(document, anchor),
    };
    let last = span.until.map_or(usize::MAX, |u| u.page);
    let mut script = Script::new(skip_folios);
    // The page before, for a sentence that started on it.
    let mut before: Option<(usize, TextLayer)> = None;
    for page in first..document.page_count().min(last.saturating_add(1)) {
        let Ok(layer) = document.text_layer(page) else {
            continue;
        };
        let mut known = counts.lock().unwrap_or_else(PoisonError::into_inner);
        known.insert(page, layer.text.len());
        drop(known);
        // A selection's last page is read only up to its end.
        let upto = match span.until {
            Some(u) if u.page == page => u.char,
            _ => usize::MAX,
        };
        let chars = layer.text.chars().iter().take(upto);
        let glyphs: Vec<Glyph> = chars
            .map(|c| Glyph {
                ch: c.ch,
                line: c.line,
                space_before: c.space_before,
                wraps: layer.text.wraps(c.line),
            })
            .collect();
        let from = if page == first { from } else { 0 };
        for sentence in script.page(page, &glyphs, from) {
            let start = sentence.start;
            let on = match start.page == page {
                true => Some(&layer),
                false => before
                    .as_ref()
                    .filter(|(p, _)| *p == start.page)
                    .map(|(_, l)| l),
            };
            let anchor = on.and_then(|l| l.place(start.char)).map(anchor);
            let parts = on.map_or_else(Vec::new, |l| parts::at(l, start.char));
            let said = Said {
                sentence,
                anchor,
                parts,
            };
            if tx.send(said).is_err() {
                return;
            }
            wake();
        }
        before = Some((page, layer));
    }
    for sentence in script.finish() {
        if tx
            .send(Said {
                sentence,
                anchor: None,
                parts: Vec::new(),
            })
            .is_err()
        {
            return;
        }
    }
    wake();
}

fn anchor((block, offset): (usize, usize)) -> Anchor {
    Anchor { block, offset }
}

/// The page and character where `anchor` is drawn now.
fn locate(document: &Document, at: Anchor) -> (usize, usize) {
    let page = document.locate(at).unwrap_or_default();
    let Ok(layer) = document.text_layer(page) else {
        return (page, 0);
    };
    let count = layer.text.len();
    let char = (0..count).find(|&i| layer.place(i).is_some_and(|p| anchor(p) >= at));
    (page, char.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use mareader_pdf::sample::pdf;
    use mareader_session::{Budget, Event, Session, Source};

    use super::*;

    /// The in-memory sample, opened here so no worker has to be waited on.
    fn document() -> Option<Arc<Document>> {
        let mut session = Session::new(0, || {});
        let bytes = Arc::new(pdf(1, "t"));
        session.open(Source::Bytes(bytes), None, Default::default());
        let budget = Budget {
            items: 1,
            bytes: usize::MAX,
        };
        let id = session.poll(budget).into_iter().find_map(|e| match e {
            Event::Opened { result: Ok(id), .. } => Some(id),
            _ => None,
        })?;
        Some(Arc::clone(&session.get(id)?.doc))
    }

    #[test]
    fn about_twenty_seconds_of_speech_is_made_ahead() {
        let short = ["Yes."; 9];
        assert_eq!(enough(short.into_iter()), Some(LOOK_MOST), "never more");
        let long = "a".repeat(LOOK_CHARS);
        let texts = [long.as_str(), "Yes.", "Yes.", "Yes."];
        assert_eq!(enough(texts.into_iter()), Some(LOOK), "never fewer");
        let some = "a".repeat(LOOK_CHARS / 5);
        assert_eq!(enough([some.as_str(); 9].into_iter()), Some(5));
        assert_eq!(enough(["Yes."; 4].into_iter()), None, "wants more");
    }

    #[test]
    fn a_thread_reads_the_sample_and_wakes_the_window() -> Result<(), &'static str> {
        let document = document().ok_or("the in-memory sample didn't open")?;
        let wakes = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&wakes);
        let wake: Waker = Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });
        let counts = Counts::default();
        let shared = Arc::clone(&counts);
        let span = Reach {
            start: Start::Spot(Spot { page: 0, char: 0 }),
            until: None,
        };
        let feed = spawn(document, span, false, (wake, counts));
        // The thread drops its sender when it ends, so this cannot hang.
        while feed.recv().is_ok() {}
        assert!(
            wakes.load(Ordering::SeqCst) > 0,
            "the reader wakes the window it was given"
        );
        let pages: Vec<usize> = shared
            .lock()
            .map_or_else(|_| Vec::new(), |m| m.keys().copied().collect());
        assert_eq!(pages, [0], "the page it read, with its length counted");
        Ok(())
    }

    #[test]
    fn an_anchor_is_found_on_the_page_it_is_drawn_on() -> Result<(), &'static str> {
        let document = document().ok_or("the in-memory sample didn't open")?;
        let at = Anchor {
            block: 0,
            offset: 0,
        };
        assert_eq!(locate(&document, at), (0, 0));
        Ok(())
    }
}
