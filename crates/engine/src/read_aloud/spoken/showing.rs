//! What a reading shows and asks: the words lit, the lyrics, the time left.

use std::ops::Range;
use std::sync::PoisonError;

use mareader_settings::Highlight;
use mareader_speech::Spot;

use super::Spoken;
use crate::read_aloud::lit::{Lit, Span};
use crate::read_aloud::parts::Part;
use crate::read_aloud::place::place_name;
use crate::read_aloud::player::{Lyrics, View};
use crate::read_aloud::words;

impl Spoken {
    pub fn lit(&self) -> Option<Lit> {
        let (start, end) = self
            .lit_spots()
            .filter(|_| !self.finished && !self.relaid)?;
        Some(Lit {
            span: Span::Chars { start, end },
        })
    }

    /// Where the words lit start and end: the whole sentence until heard.
    pub(super) fn lit_spots(&self) -> Option<(Spot, Spot)> {
        let said = &self.current.as_ref()?.sentence;
        let whole = Some((said.start, said.end));
        if self.highlight.0 == Highlight::Sentence || said.words.is_empty() {
            return whole;
        }
        let lit = self.lit_words(self.highlight)?;
        let first = said.words.get(lit.start)?;
        let last = said.words.get(lit.end.checked_sub(1)?)?;
        Some((first.start, last.end))
    }

    /// The words of the sentence said lit as `highlight` asks, by index.
    fn lit_words(&self, highlight: (Highlight, u8)) -> Option<Range<usize>> {
        let said = &self.current.as_ref()?.sentence;
        let heard = self.heard.filter(|_| self.saying);
        // The voice's own clock, else ours: either way, pauses left out.
        let seconds = match heard {
            Some(h) => h.seconds,
            None if self.timed => 0.0,
            None => self.clock.seconds(),
        };
        let guess = self.pace.bytes(seconds, self.tone.rate);
        let at = words::heard(&said.words, heard.and_then(|h| h.word), guess);
        Some(words::group(&said.text, &said.words, at, highlight))
    }

    /// The sentences round the one said; phrases lit for page sentences.
    pub fn lyrics(&self) -> Option<Lyrics> {
        let said = &self.current.as_ref().filter(|_| !self.finished)?.sentence;
        let highlight = match self.highlight.0 {
            Highlight::Sentence => (Highlight::Phrases, 1),
            _ => self.highlight,
        };
        let lit = self.lit_words(highlight).unwrap_or_default();
        let preferred = self.lit_words(self.highlight).unwrap_or_default();
        let heard = self.lit_words((Highlight::Words, 1)).map_or(0, |r| r.start);
        let start = said.start;
        Some(Lyrics {
            before: self.behind.back().map(|s| s.sentence.text.clone()),
            now: said.text.clone(),
            words: said.words.iter().map(|w| w.bytes.clone()).collect(),
            lit,
            preferred,
            heard,
            after: self.ahead.front().map(|s| s.sentence.text.clone()),
            key: (start.page as u64) << 32 | start.char as u64,
        })
    }

    /// The page and chapter read, as the card names them.
    pub fn whereabouts(&self) -> Option<String> {
        let (start, _) = self.lit_spots()?;
        Some(place_name(&self.document, start.page))
    }

    /// The pane, the page read, and the characters lit on it.
    pub fn spot(&self) -> Option<(usize, Range<usize>)> {
        let (start, end) = self
            .lit_spots()
            .filter(|_| !self.finished && !self.relaid)?;
        let last = if end.page == start.page {
            end.char
        } else {
            start.char + 1
        };
        Some((start.page, start.char..last.max(start.char + 1)))
    }

    /// What the player shows: sentences for steps, pages for time.
    pub fn view<'a>(&self, hint: &'a dyn Fn(f32) -> Option<String>) -> View<'a> {
        let can = self.voicer.is_some();
        let progress = self.progress();
        let note = match (&self.fault, self.finished) {
            (Some(fault), _) => Some((fault.clone(), true)),
            (None, true) if self.chosen.is_some() => Some(("Selection read".to_owned(), false)),
            (None, true) => Some(("The end".to_owned(), false)),
            (None, false) => None,
        };
        View {
            playing: self.playing(),
            ready: can,
            loading: self.voicer.as_ref().is_some_and(|v| v.loading()),
            back: !self.behind.is_empty() && can,
            next: !self.finished && can,
            unit: "sentence",
            escape: (self.current.as_ref())
                .and_then(|s| s.parts.last())
                .map(|p| p.kind)
                .filter(|_| can && !self.finished && self.asking.is_none()),
            ask: self.asking.filter(|_| can).map(|p| p.kind),
            at: progress.map(|p| p.0),
            left: progress.map(|p| p.1),
            total: progress.map(|p| p.2),
            seek: can && self.chosen.is_none(),
            note,
            hint,
            narrated: false,
        }
    }

    /// How much is read, and seconds left and in all, from pages' lengths.
    fn progress(&self) -> Option<(f32, f64, f64)> {
        let counts = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
        let usual = counts.values().sum::<usize>() as f64 / counts.len().max(1) as f64;
        if counts.is_empty() || usual < 1.0 {
            return None;
        }
        // A spot as pages read: its page, and the share of it before.
        let pages = |s: Spot| {
            let on = counts.get(&s.page).map_or(usual, |&n| n as f64).max(1.0);
            s.page as f64 + (s.char as f64 / on).min(1.0)
        };
        let (from, to) = match self.chosen {
            Some((_, until)) => (pages(self.began?), pages(until)),
            None => (0.0, self.document.page_count() as f64),
        };
        let here = match self.finished {
            true => to,
            false => self.lit_spots().map_or(from, |(start, _)| pages(start)),
        };
        let here = here.clamp(from, to.max(from));
        let span = (to - from).max(1e-6);
        let speed = self.steady.bytes(1.0, self.tone.rate).max(1.0);
        let seconds = |pages: f64| pages * usual / speed;
        Some((
            ((here - from) / span) as f32,
            seconds(to - here),
            seconds(span),
        ))
    }

    /// The page at `share` of the book.
    pub(super) fn page_at(&self, share: f32) -> usize {
        let count = self.document.page_count();
        ((share * count as f32) as usize).min(count.saturating_sub(1))
    }

    pub fn hint(&self, share: f32) -> Option<String> {
        let none = self.chosen.is_some();
        (!none).then(|| place_name(&self.document, self.page_at(share)))
    }

    /// Holds at a part's start, asking whether to read it.
    pub(super) fn ask(&mut self, part: Part) {
        self.asking = Some(part);
        self.paused = true;
        self.clock.stop();
    }

    /// Estimate only when the buffered sentences include the part's end.
    pub fn ask_cost(&self, rate: f32) -> Option<String> {
        let part = self.asking?;
        let first = self.current.as_ref()?;
        let mut bytes = first.sentence.text.len();
        let mut ended = false;
        for said in &self.ahead {
            if !said.parts.contains(&part) {
                ended = true;
                break;
            }
            bytes = bytes.saturating_add(said.sentence.text.len());
        }
        if !ended && self.ahead.is_empty() {
            return None;
        }
        let seconds = (bytes as f32 / (14.0 * rate.clamp(0.5, 3.0)))
            .ceil()
            .max(1.0);
        let prefix = if ended { "~" } else { "at least ~" };
        Some(format!("{prefix}{} s", seconds as u32))
    }

    pub fn is_asking(&self) -> bool {
        self.asking.is_some()
    }

    /// The answer to the part asked about: read it, or pass it over.
    pub(super) fn answer(&mut self, read: bool) {
        let Some(part) = self.asking.take() else {
            return;
        };
        self.paused = false;
        if read {
            self.cleared.push(part);
        } else {
            self.leaving = Some(part);
            self.pass();
        }
        self.go_on();
    }
}
