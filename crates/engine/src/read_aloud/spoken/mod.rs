//! One book said in one pane: its state, and how it is opened and set.
//!
//! The front end opens one, ticks it each frame, and asks what it shows. It
//! names no window type: a `Waker` and a `Repaint` ask for its frames, `Stand`
//! says where the book is open, and `Panes` turns and lights that pane.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use mareader_session::{Anchor, DocId, Document};
use mareader_settings::{Highlight, Pass, ReadAloudSettings, Skips};
use mareader_speech::{Heard, Shelf, Spot, Tone, Voicer, Waker};
use mareader_workspace::PaneId;

use super::feed::{Counts, Reach, Said, Start, spawn};
use super::parts::{self, Part};
use super::place::Stand;
use super::tone;
use super::words::{Clock, Pace, Steady};
use crate::repaint::Repaint;

mod saying;
mod showing;

/// Sentences kept behind the voice, for stepping back.
const BEHIND: usize = 200;

/// One book being said in one pane.
pub struct Spoken {
    pub pane: PaneId,
    doc: DocId,
    document: Arc<Document>,
    voicer: Option<Box<dyn Voicer>>,
    wake: Waker,
    /// Asks the window for a frame once a tick has passed.
    repaint: Repaint,
    feed: Option<Receiver<Said>>,
    behind: VecDeque<Said>,
    /// The sentence said now, or next once the voice is free.
    current: Option<Said>,
    /// Sentences after the current: stepped back over, or read ahead.
    ahead: VecDeque<Said>,
    /// The voice's id for `current`, and whether it is saying it.
    id: u64,
    saying: bool,
    paused: bool,
    finished: bool,
    fault: Option<String>,
    engine: String,
    /// The model build opened, for a model's engine.
    build: Option<&'static str>,
    voice: Option<String>,
    tone: Tone,
    /// The model's own settings, as last given to it.
    knobs: std::collections::BTreeMap<String, f32>,
    skip_folios: bool,
    followed: Option<usize>,
    /// The sentence last brought into view.
    revealed: Option<Spot>,
    /// Set anew: the sentence said is being found on the new pages.
    relaid: bool,
    /// A selection read alone: where it began, and where it stops.
    chosen: Option<(Start, Spot)>,
    /// How much is lit, and words at a time.
    highlight: (Highlight, u8),
    /// Where the voice says it is in the sentence said.
    heard: Option<Heard>,
    /// The voice has said where it is: till it does again, it is starting.
    timed: bool,
    clock: Clock,
    pace: Pace,
    /// The pace over the whole reading, for the time left.
    steady: Steady,
    counts: Counts,
    /// Where the first sentence since reading began starts.
    began: Option<Spot>,
    /// Sought: the pane turns to this page, following or not.
    jump: Option<usize>,
    /// Freshly found again in a pane, which should turn to the words read.
    homing: bool,
    /// The part being skipped: its sentences are passed unsaid.
    leaving: Option<Part>,
    /// What reading does on reaching each kind of part.
    skips: Skips,
    /// Explicit overrides belonging only to this book.
    pub book_mask: u8,
    /// The parts read on into: they are not asked about again.
    cleared: Vec<Part>,
    /// The part asked about, held at its start for an answer.
    asking: Option<Part>,
    /// Read afresh: the parts it starts in are read unasked.
    entered: bool,
}

impl Spoken {
    pub fn new(
        repaint: Repaint,
        reading: &Stand,
        (start, until): (Start, Option<Spot>),
        (settings, shelf): (&ReadAloudSettings, &Shelf),
    ) -> Self {
        let kept = repaint.clone();
        let wake: Waker = Arc::new(move || kept.now());
        let mut spoken = Self {
            pane: reading.pane,
            doc: reading.doc,
            document: Arc::clone(&reading.document),
            voicer: None,
            wake,
            repaint,
            feed: None,
            behind: VecDeque::new(),
            current: None,
            ahead: VecDeque::new(),
            id: 0,
            saying: false,
            paused: false,
            finished: false,
            fault: None,
            engine: String::new(),
            build: None,
            voice: None,
            tone: Tone::default(),
            knobs: std::collections::BTreeMap::new(),
            skip_folios: settings.skip_page_numbers,
            followed: None,
            revealed: None,
            relaid: false,
            chosen: until.map(|until| (start, until)),
            highlight: (settings.highlight, settings.highlight_words),
            heard: None,
            timed: false,
            clock: Clock::default(),
            pace: Pace::default(),
            steady: Steady::default(),
            counts: Counts::default(),
            began: None,
            jump: None,
            homing: false,
            leaving: None,
            skips: settings.skips,
            book_mask: 0,
            cleared: Vec::new(),
            asking: None,
            entered: true,
        };
        spoken.open(settings, shelf);
        spoken.read_from(start);
        spoken
    }

    /// True when it reads `reading`'s book in its pane.
    pub fn reads(&self, reading: &Stand) -> bool {
        self.pane == reading.pane && self.doc == reading.doc
    }

    /// Reads again from `start`, as asked from the page's menu.
    pub fn restart(
        &mut self,
        reading: &Stand,
        (start, until): (Start, Option<Spot>),
        on: (&ReadAloudSettings, &Shelf),
    ) {
        self.document = Arc::clone(&reading.document);
        self.chosen = until.map(|until| (start, until));
        self.apply(on);
        self.paused = false;
        self.read_from(start);
    }

    /// Where the sentence said starts in the book, while it is placed.
    pub fn said(&self) -> Option<Anchor> {
        self.current.as_ref().filter(|_| !self.relaid)?.anchor
    }

    /// The engine read with.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn engine(&self) -> &str {
        &self.engine
    }

    /// Starts the engine settings name, keeping its fault if it fails.
    fn open(&mut self, settings: &ReadAloudSettings, shelf: &Shelf) {
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
        self.engine = settings.engine.clone();
        self.build = mareader_speech::build_of(&self.engine, settings.build(), shelf);
        self.voice = settings.voice().map(str::to_owned);
        self.tone = tone(settings);
        self.knobs = settings.knobs();
        let wake = Arc::clone(&self.wake);
        match mareader_speech::open(&self.engine, self.build, shelf, wake) {
            Ok(mut voicer) => {
                voicer.set_voice(self.voice.as_deref());
                voicer.set_tone(self.tone);
                voicer.set_knobs(&self.knobs);
                self.voicer = Some(voicer);
                self.fault = None;
            }
            Err(fault) => {
                self.voicer = None;
                self.fault = Some(fault.to_string());
            }
        }
        self.saying = false;
    }

    /// Drops what was read and reads the pages afresh from `start`.
    pub(super) fn read_from(&mut self, start: Start) {
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
        self.saying = false;
        self.finished = false;
        self.followed = None;
        self.revealed = None;
        self.relaid = false;
        self.behind.clear();
        self.ahead.clear();
        self.current = None;
        self.began = None;
        self.leaving = None;
        self.cleared.clear();
        self.entered = true;
        if self.asking.take().is_some() {
            self.paused = false;
        }
        // No voice, nothing worth reading for.
        let until = self.chosen.map(|(_, until)| until);
        self.feed = self.voicer.as_ref().map(|_| {
            let document = Arc::clone(&self.document);
            let span = Reach { start, until };
            let to = (Arc::clone(&self.wake), Arc::clone(&self.counts));
            spawn(document, span, self.skip_folios, to)
        });
    }

    /// Takes up changed settings; a changed voice says the sentence again.
    pub(super) fn apply(&mut self, (settings, shelf): (&ReadAloudSettings, &Shelf)) {
        self.highlight = (settings.highlight, settings.highlight_words);
        self.skips = settings.skips;
        for kind in parts::Kind::ALL {
            if self.book_mask & kind.bit() != 0 {
                kind.set_skip(&mut self.skips);
            }
        }
        // Asked no more, the question answers itself.
        match self.asking.map(|p| p.kind.pass(&self.skips)) {
            Some(Pass::Read) => self.answer(true),
            Some(Pass::Skip) => self.answer(false),
            _ => {}
        }
        let build = mareader_speech::build_of(&settings.engine, settings.build(), shelf);
        if settings.engine != self.engine || build != self.build {
            let at = self.place();
            self.open(settings, shelf);
            if let Some(at) = at {
                self.read_from(at);
            }
            return;
        }
        let voice = settings.voice().map(str::to_owned);
        let tone = tone(settings);
        let Some(v) = self.voicer.as_mut() else {
            return;
        };
        let mut again = false;
        if voice != self.voice {
            // Another voice keeps its own pace.
            self.steady = Steady::default();
            v.set_voice(voice.as_deref());
            self.voice = voice;
            again = true;
        }
        if tone != self.tone {
            v.set_tone(tone);
            self.tone = tone;
            again = true;
        }
        let knobs = settings.knobs();
        if knobs != self.knobs {
            v.set_knobs(&knobs);
            self.knobs = knobs;
            again = true;
        }
        // Paused mid-sentence, it is said anew in the new voice on play.
        if again && self.saying && self.paused {
            v.stop();
            self.saying = false;
        } else if again && self.saying {
            self.say();
        }
        if settings.skip_page_numbers != self.skip_folios {
            self.skip_folios = settings.skip_page_numbers;
            let at = self.current.as_ref().map(|s| Start::Spot(s.sentence.start));
            if let Some(at) = at {
                self.read_from(at);
            }
        }
    }

    /// Where the sentence said now starts, by place where it has one.
    fn place(&self) -> Option<Start> {
        let said = self.current.as_ref()?;
        Some(match said.anchor {
            Some(anchor) => Start::Anchor(anchor),
            None => Start::Spot(said.sentence.start),
        })
    }

    /// Whether `reading` shows this book, perhaps set in new type.
    pub fn ours(&self, reading: &Stand) -> bool {
        let same = reading.doc == self.doc || reading.document.same_book(&self.document);
        reading.document.narration().is_none() && same
    }

    /// Reads on in `reading`, the book opened again; false if it differs.
    pub fn rehome(&mut self, reading: &Stand) -> bool {
        if reading.document.narration().is_some() {
            return false;
        }
        let fixed = self.document.layout().is_none();
        let same = reading.document.layout() == self.document.layout()
            && reading.document.page_count() == self.document.page_count();
        if fixed && !same {
            return false;
        }
        self.pane = reading.pane;
        (self.followed, self.revealed) = (None, None);
        self.homing = true;
        // Known by its new doc from now on, else it seems away again.
        self.doc = reading.doc;
        // Pages laid alike are the same; others are found anew in `tick`.
        if same {
            self.document = Arc::clone(&reading.document);
        }
        true
    }
}

impl Drop for Spoken {
    fn drop(&mut self) {
        // Some systems speak on after their handle is gone.
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
    }
}

#[cfg(test)]
mod tests;

/// A table asked about, and what each of its two answers does.
#[cfg(test)]
#[path = "asking_tests.rs"]
mod asking;
