//! Narrated-book read-aloud playback using embedded recordings.

use std::sync::Arc;
use std::time::Duration;

use mareader_audio::{Fault, Player, Segment};
use mareader_epub::{Narration, Run, Skips};
use mareader_session::{Anchor, Document};
use mareader_settings::ReadAloudSettings;
use mareader_workspace::PaneId;

use super::TICK;
use super::lit::{Lit, Span};
use super::parts;
use super::place::{Panes, Stand, follow, place_name};
use super::player::{Act, View};
use crate::play::fetch;

/// A narrated book being read aloud in one pane: its own recordings.
pub(crate) struct Narrated {
    pub(crate) pane: PaneId,
    pub(crate) narration: Arc<Narration>,
    pub(crate) document: Arc<Document>,
    /// Seconds of sound before each phrase, and in all at the end.
    lengths: Vec<f64>,
    /// Sought: the pane turns to the phrase, following or not.
    jump: bool,
    player: Player,
    /// What was last asked to play, a run per segment tag.
    runs: Vec<Run>,
    /// The phrase heard last; where playing last started.
    pub(crate) heard: Option<usize>,
    started: usize,
    /// The page last turned to, so the reader may look away between.
    followed: Option<usize>,
    skip: Skips,
    rate: f32,
    pub(crate) paused: bool,
    pub(crate) finished: bool,
    fault: Option<Fault>,
}

impl Narrated {
    pub(crate) fn new(
        stand: &Stand,
        narration: Arc<Narration>,
        first: usize,
        settings: &ReadAloudSettings,
    ) -> Option<Self> {
        let files = stand.document.files()?;
        let mut s = Self {
            pane: stand.pane,
            lengths: lengths(&narration),
            narration,
            document: Arc::clone(&stand.document),
            jump: false,
            player: Player::new(fetch(files)),
            runs: Vec::new(),
            heard: None,
            started: first,
            followed: None,
            skip: skips(settings),
            rate: settings.rate,
            paused: false,
            finished: false,
            fault: None,
        };
        s.play_from(first);
        Some(s)
    }

    pub(crate) fn restart(&mut self, pane: PaneId, first: usize, settings: &ReadAloudSettings) {
        self.pane = pane;
        self.skip = skips(settings);
        self.rate = settings.rate;
        self.play_from(first);
    }

    fn play_from(&mut self, from: usize) {
        self.runs = self.narration.runs(from, self.skip);
        let segments = (self.runs.iter().enumerate())
            .map(|(tag, run)| Segment {
                path: run.path.clone(),
                from: run.from,
                until: run.until,
                tag: tag as u64,
            })
            .collect();
        self.player.set_rate(self.rate);
        self.player.play(segments);
        self.heard = Some(from);
        self.started = from;
        self.followed = None;
        self.finished = false;
        self.paused = false;
        self.fault = None;
    }

    /// The first phrase read at or after `i`.
    fn read_from(&self, i: usize) -> Option<usize> {
        match self.narration.reads(i, self.skip) {
            true => Some(i),
            false => self.narration.next(i, self.skip),
        }
    }

    pub(crate) fn toggle(&mut self) {
        if self.finished {
            if let Some(first) = self.read_from(0) {
                self.play_from(first);
            }
        } else if self.paused {
            self.player.resume();
            self.paused = false;
        } else {
            self.player.pause();
            self.paused = true;
        }
    }

    pub(crate) fn pause(&mut self) {
        if !self.paused && !self.finished {
            self.player.pause();
            self.paused = true;
        }
    }

    fn step(&mut self, to: Option<usize>) {
        if let Some(to) = to {
            self.play_from(to);
        }
    }

    /// Follows what is heard and turns `here`, its pane, if shown.
    pub(crate) fn tick(
        &mut self,
        panes: &mut dyn Panes,
        here: Option<&Stand>,
        settings: &ReadAloudSettings,
    ) -> (bool, Option<Duration>) {
        if let Some(stand) = here {
            self.document = Arc::clone(&stand.document);
        }
        if (settings.rate - self.rate).abs() > f32::EPSILON {
            self.rate = settings.rate;
            self.player.set_rate(self.rate);
        }
        let status = self.player.status();
        let run = |tag: u64| usize::try_from(tag).ok().and_then(|t| self.runs.get(t));
        if let Some((run, time)) = status.at.and_then(|(tag, time)| Some((run(tag)?, time))) {
            self.heard = self.narration.phrase_at(run, time).or(self.heard);
        }
        self.paused = status.paused;
        self.finished = status.done && status.fault.is_none() && !self.runs.is_empty();
        self.fault = status.fault;
        let skip = skips(settings);
        if skip != self.skip {
            self.skip = skip;
            let from = self.heard.unwrap_or(self.started);
            if let Some(from) = self.read_from(from).filter(|_| !self.finished) {
                self.play_from(from);
            }
        }
        let shown = self.heard.and_then(|i| self.narration.phrases.get(i));
        let page = shown.and_then(|p| self.document.locate(p.start));
        // Away from its pane, the book reads on unseen.
        if let Some(stand) = here {
            if std::mem::take(&mut self.jump)
                && let Some(page) = page
            {
                panes.go_to(self.pane, page);
                self.followed = Some(page);
            }
            follow(panes, stand, settings, page, &mut self.followed);
        }
        let wait = (!self.paused && !self.finished).then_some(TICK);
        (true, wait)
    }

    /// What the player shows: phrases for steps, the recordings for time.
    pub(crate) fn view<'a>(&self, hint: &'a dyn Fn(f32) -> Option<String>) -> View<'a> {
        let heard = self.heard.unwrap_or(self.started);
        let count = self.narration.phrases.len().max(1);
        let total = self.lengths.last().copied().unwrap_or_default();
        let before = self.lengths.get(heard).copied().unwrap_or(total);
        let rate = f64::from(self.rate.max(0.1));
        let note = match (&self.fault, self.finished) {
            (Some(fault), _) => Some((fault_text(fault), true)),
            (None, true) => Some(("The end".to_owned(), false)),
            (None, false) => None,
        };
        View {
            playing: !self.paused && !self.finished,
            ready: true,
            loading: false,
            back: self.narration.previous(heard, self.skip).is_some(),
            next: self.narration.next(heard, self.skip).is_some(),
            unit: "phrase",
            escape: (self.narration.escape(heard, self.skip)).map(|_| parts::Kind::Table),
            ask: None,
            at: Some(if self.finished {
                1.0
            } else {
                heard as f32 / count as f32
            }),
            left: Some(if self.finished {
                0.0
            } else {
                (total - before) / rate
            }),
            total: Some(total / rate),
            seek: true,
            note,
            hint,
            narrated: true,
        }
    }

    /// The phrase at `share` of the book.
    fn phrase_at(&self, share: f32) -> usize {
        let count = self.narration.phrases.len();
        ((share * count as f32) as usize).min(count.saturating_sub(1))
    }

    pub(crate) fn hint(&self, share: f32) -> Option<String> {
        let phrase = self.narration.phrases.get(self.phrase_at(share))?;
        let page = self.document.locate(phrase.start)?;
        Some(place_name(&self.document, page))
    }

    /// Does what the player asked; true to stop.
    pub(crate) fn act(&mut self, act: Act) -> bool {
        let heard = self.heard.unwrap_or(self.started);
        match act {
            Act::Back => self.step(self.narration.previous(heard, self.skip)),
            Act::Next => self.step(self.narration.next(heard, self.skip)),
            Act::Escape => self.step(self.narration.escape(heard, self.skip)),
            Act::Toggle => self.toggle(),
            Act::Seek(share) => {
                let to = self.read_from(self.phrase_at(share));
                self.jump = to.is_some();
                self.step(to);
            }
            Act::Stop => return true,
            Act::Card(_) | Act::Reveal | Act::Answer(_) => {}
        }
        false
    }

    /// Whether `stand` is this narration's book, as it was opened.
    pub(crate) fn ours(&self, stand: &Stand) -> bool {
        let narration = stand.document.narration();
        narration.is_some_and(|n| Arc::ptr_eq(&n, &self.narration))
    }

    /// Reads on in `stand`, the book opened again; false if it differs.
    pub(crate) fn rehome(&mut self, stand: &Stand) -> bool {
        let Some(narration) = stand.document.narration() else {
            return false;
        };
        // Its phrases' places are the book's own, whatever the type.
        if narration.phrases.len() != self.narration.phrases.len() {
            return false;
        }
        self.narration = narration;
        self.document = Arc::clone(&stand.document);
        self.pane = stand.pane;
        (self.jump, self.followed) = (true, None);
        true
    }

    /// The page the phrase heard is on, and where it is named.
    pub(crate) fn page(&self) -> Option<usize> {
        let phrase = self.narration.phrases.get(self.heard?)?;
        self.document.locate(phrase.start)
    }

    pub(crate) fn lit(&self) -> Option<Lit> {
        let phrase = self.narration.phrases.get(self.heard?)?;
        Some(Lit {
            span: Span::Anchors {
                start: phrase.start,
                end: phrase.end,
            },
        })
    }

    pub(crate) fn said(&self) -> Option<Anchor> {
        let phrase = self.narration.phrases.get(self.heard?)?;
        Some(phrase.start)
    }
}

/// Seconds of sound before each phrase, ending with the whole.
fn lengths(narration: &Narration) -> Vec<f64> {
    let known: Vec<f64> = (narration.phrases.iter())
        .filter_map(|p| Some(p.audio.end? - p.audio.begin))
        .collect();
    // A phrase played to its file's end counts as the usual length.
    let usual = match known.len() {
        0 => 3.0,
        n => known.iter().sum::<f64>() / n as f64,
    };
    let mut out = Vec::with_capacity(narration.phrases.len() + 1);
    let mut sum = 0.0;
    out.push(sum);
    for p in &narration.phrases {
        sum += p
            .audio
            .end
            .map_or(usual, |end| end - p.audio.begin)
            .max(0.0);
        out.push(sum);
    }
    out
}

fn fault_text(fault: &Fault) -> String {
    match fault {
        Fault::NoDevice => "No sound output".to_owned(),
        Fault::Unreadable(_) => "A sound file won't play".to_owned(),
    }
}

/// What `settings` leaves out.
pub(crate) fn skips(settings: &ReadAloudSettings) -> Skips {
    Skips {
        notes: settings.skip_notes,
        page_numbers: settings.skip_page_numbers,
    }
}
