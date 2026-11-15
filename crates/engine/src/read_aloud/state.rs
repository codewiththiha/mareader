//! Engine-owned read-aloud state: playback, voice booths, and model shelf.

use std::ops::Range;
use std::sync::Arc;

use mareader_library::BookId;
use mareader_session::Anchor;
use mareader_settings::ReadAloudSettings;
use mareader_speech::{Item, Shelf, Spot, Voice};
use mareader_workspace::PaneId;

use super::ReadFrom;
use super::active::Active;
pub use super::active::{About, Origin};
use super::booth::Booth;
use super::feed::Start;
use super::lit::Lit;
use super::narrated::{Narrated, skips};
use super::parts;
use super::place::{Panes, Stand};
use super::player::{Act, Lyrics, View};
use super::spoken::Spoken;
pub use super::voices::{Offer, Speaker};
use crate::dict::Download;
use crate::repaint::Repaint;

/// Seconds after a card's word before its engine is let go.
const WORD_IDLE: f64 = 60.0;

/// Engine-owned read-aloud playback, voice booths, and speech model shelf.
#[derive(Default)]
pub struct State {
    active: Option<Active>,
    /// Voices to pick from and try, while settings show them.
    pub booth: Booth,
    /// Voice models on offer, downloaded or coming down.
    pub shelf: Shelf,
    /// Says a card's word in the voice books are read in.
    pub(super) word: Booth,
    /// An engine's voices as last listed, kept once its booths close.
    pub(super) known: (String, Vec<Voice>),
    /// Engines opened once only to list their voices.
    pub(super) listed_once: Vec<String>,
    /// The word being said, and when one was last asked.
    saying: Option<String>,
    said_at: f64,
    /// The book read, to find it again once its pane shows another.
    origin: Option<Origin>,
    origin_at: Option<PaneId>,
    /// Its pane shows another book, or is gone: it reads on unseen.
    away: bool,
    repaint: Repaint,
}

impl State {
    pub fn attach_repaint(&mut self, repaint: Repaint) {
        self.repaint = repaint.clone();
        self.booth.attach(repaint.clone());
        self.word.attach(repaint.clone());
        if let Some(Active::Narrated(s)) = &mut self.active {
            s.set_repaint(repaint);
        }
    }

    /// Does what a voice-model button asked; a failed delete says why.
    pub fn act_model(
        &mut self,
        item: Item,
        action: Download,
        settings: &mut ReadAloudSettings,
    ) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        match action {
            Download::Start => self.shelf.download(item),
            Download::Pause => self.shelf.pause(item),
            Download::Cancel => self.shelf.cancel(item),
            Download::Remove => {
                let removed = self.shelf.remove(item);
                // Samples may hold the model; the booth opens again when shown.
                self.booth.close();
                self.stop_word();
                self.word.close();
                self.let_go(settings);
                return removed;
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (item, action, settings);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn let_go(&mut self, settings: &mut ReadAloudSettings) {
        let engine = settings.engine.as_str();
        let shelf = &self.shelf;
        let gone = shelf.model(engine).is_some() && !shelf.ready().iter().any(|m| m.id == engine);
        if !gone {
            return;
        }
        if let Some(Active::Spoken(s)) = &self.active
            && s.engine() == engine
        {
            self.stop();
        }
        settings.engine = mareader_speech::SYSTEM.to_owned();
    }

    pub fn active(&self) -> bool {
        self.active.is_some()
    }

    pub fn playing(&self) -> bool {
        self.active.as_ref().is_some_and(Active::playing)
    }

    pub fn hush(&mut self) {
        self.pause();
        self.booth.stop_trying();
        self.stop_word();
    }

    pub fn toggle(&mut self) -> bool {
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        active.toggle();
        true
    }

    pub fn pause(&mut self) {
        if let Some(active) = self.active.as_mut() {
            active.pause();
        }
    }

    pub fn stop(&mut self) {
        self.active = None;
    }

    /// Starts reading from `stand`; returns whether playback was inactive.
    pub fn start(&mut self, stand: &Stand, asked: ReadFrom, settings: &ReadAloudSettings) -> bool {
        let fresh = self.active.is_none();
        let (from, at, until) = (asked.anchor, asked.at, asked.until);
        self.away = false;
        if self.home() != Some(stand.pane) || self.active.as_ref().is_some_and(|s| !s.ours(stand)) {
            self.origin = None;
        }
        if stand.document.narration().is_none() || until.is_some() {
            let start = match (from, at) {
                (Some(anchor), _) => Start::Anchor(anchor),
                (None, Some(spot)) => Start::Spot(spot),
                (None, None) => Start::Spot(Spot {
                    page: stand.current,
                    char: 0,
                }),
            };
            if let Some(Active::Spoken(s)) = self.active.as_mut()
                && s.reads(stand)
            {
                s.restart(stand, (start, until), (settings, &self.shelf));
                return fresh;
            }
            let on = (settings, &self.shelf);
            let spoken = Spoken::new(self.repaint.clone(), stand, (start, until), on);
            self.active = Some(Active::Spoken(Box::new(spoken)));
            return fresh;
        }
        let Some(narration) = stand.document.narration() else {
            return fresh;
        };
        let skip = skips(settings);
        let from = from.or_else(|| stand.document.anchor(stand.current));
        let first = from.and_then(|a| narration.from_anchor(a, skip));
        let Some(first) = first.or_else(|| narration.from_anchor(Anchor::default(), skip)) else {
            return fresh;
        };
        let same = match self.active.as_ref() {
            Some(Active::Narrated(s)) => Arc::ptr_eq(&s.narration, &narration),
            _ => false,
        };
        if same {
            if let Some(Active::Narrated(s)) = self.active.as_mut() {
                s.restart(stand.pane, first, settings);
            }
        } else if let Some(narrated) =
            Narrated::new(self.repaint.clone(), stand, narration, first, settings)
        {
            self.active = Some(Active::Narrated(narrated));
        }
        fresh
    }

    /// Advances playback and word-card booth; turns `panes` while following.
    pub fn tick(
        &mut self,
        now: f64,
        rise: f32,
        panes: &mut dyn Panes,
        stand: Option<&Stand>,
        settings: &ReadAloudSettings,
    ) {
        self.tick_word(now);
        let Some(active) = self.active.as_mut() else {
            self.origin = None;
            return;
        };
        let mask = self
            .origin
            .as_ref()
            .and_then(|o| o.book)
            .and_then(|book| settings.book_skips.get(&book).copied())
            .unwrap_or(0);
        if let Active::Spoken(s) = active {
            s.book_mask = mask;
        }
        let stand = stand.filter(|st| st.pane == active.pane() && active.ours(st));
        self.away = stand.is_none();
        let going = match active {
            Active::Narrated(s) => s.tick(panes, stand, settings),
            Active::Spoken(s) => s.tick(panes, stand, (settings, &self.shelf, rise)),
        };
        if !going {
            self.active = None;
        }
    }

    /// The pane currently playing, even if it now shows another document.
    pub fn pane(&self) -> Option<PaneId> {
        Some(self.active.as_ref()?.pane())
    }

    pub fn answer_question(
        &mut self,
        read: bool,
        remember: Option<parts::Kind>,
        settings: &mut ReadAloudSettings,
    ) -> bool {
        let Some(Active::Spoken(s)) = self.active.as_mut() else {
            return false;
        };
        if !s.is_asking() {
            return false;
        }
        if !read
            && let (Some(book), Some(kind)) = (self.origin.as_ref().and_then(|o| o.book), remember)
        {
            remember_skip(settings, book, kind);
        }
        s.act(Act::Answer(read));
        true
    }

    pub fn is_asking(&self) -> bool {
        matches!(self.active.as_ref(), Some(Active::Spoken(s)) if s.is_asking())
    }

    pub fn away(&self) -> Option<&Origin> {
        self.origin
            .as_ref()
            .filter(|_| self.away && self.active.is_some())
    }

    pub fn origin(&self) -> Option<&Origin> {
        self.origin.as_ref().filter(|_| self.active.is_some())
    }

    pub fn remember(&mut self, origin: impl FnOnce(PaneId) -> Origin) {
        let Some(pane) = self.home() else {
            return;
        };
        if self.origin.is_none() || self.origin_at != Some(pane) {
            self.origin = Some(origin(pane));
            self.origin_at = Some(pane);
        }
    }

    pub fn rehome(&mut self, stand: &Stand) -> bool {
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        let homed = active.rehome(stand);
        self.away &= !homed;
        homed
    }

    pub fn reading_spot(&self) -> Option<(usize, Option<Range<usize>>)> {
        self.active.as_ref()?.spot()
    }

    pub fn home(&self) -> Option<PaneId> {
        let s = self.active.as_ref().filter(|_| !self.away)?;
        Some(s.pane())
    }

    pub fn said(&self) -> Option<(PaneId, Anchor)> {
        let pane = self.home()?;
        Some((pane, self.active.as_ref()?.said()?))
    }

    pub fn lit(&self, pane: PaneId) -> Option<Lit> {
        self.active
            .as_ref()
            .filter(|_| self.home() == Some(pane))?
            .lit()
    }

    pub fn spot(&self) -> Option<(PaneId, usize, Option<Range<usize>>)> {
        let pane = self.home()?;
        let (page, chars) = self.active.as_ref()?.spot()?;
        Some((pane, page, chars))
    }

    pub fn hint(&self, share: f32) -> Option<String> {
        self.active.as_ref()?.hint(share)
    }

    pub fn ask_cost(&self, rate: f32) -> Option<String> {
        self.active.as_ref()?.ask_cost(rate)
    }

    pub fn view<'a>(&self, hint: &'a dyn Fn(f32) -> Option<String>) -> Option<View<'a>> {
        Some(self.active.as_ref()?.view(hint))
    }

    pub fn about<'a>(&'a self, title: &'a str) -> Option<About<'a>> {
        Some(self.active.as_ref()?.about(title))
    }

    pub fn lyrics(&self) -> Option<Lyrics> {
        self.active.as_ref()?.lyrics()
    }

    pub fn act_player(&mut self, act: Act) {
        if self.active.as_mut().is_some_and(|s| s.act(act)) {
            self.active = None;
        }
    }

    /// Says `word` in the engine books are read in, in `voice` or picked.
    pub fn say_word(
        &mut self,
        now: f64,
        settings: &ReadAloudSettings,
        (word, voice): (&str, Option<&str>),
    ) -> Result<(), String> {
        self.word.open(&settings.engine, settings, &self.shelf);
        self.word.say_as(settings, voice, word);
        self.said_at = now;
        if let Some(why) = self.word.fault() {
            self.saying = None;
            return Err(format!("Couldn't say “{word}”: {why}"));
        }
        self.saying = self.word.trying().then(|| word.to_owned());
        Ok(())
    }

    pub fn stop_word(&mut self) {
        self.word.stop_trying();
        self.saying = None;
    }

    pub fn saying(&self) -> Option<&str> {
        self.saying.as_deref()
    }

    pub fn words_said(&self) -> u64 {
        self.word.samples()
    }

    fn tick_word(&mut self, now: f64) {
        self.keep_listing();
        if !self.word.is_open() {
            return;
        }
        self.word.tick();
        if !self.word.trying() {
            self.saying = None;
        }
        let idle = now - self.said_at;
        if self.saying.is_none() && idle > WORD_IDLE {
            self.word.close();
        }
    }
}

/// Remembers only an explicit skip, never changing another book's policy.
pub fn remember_skip(settings: &mut ReadAloudSettings, book: BookId, kind: parts::Kind) {
    if settings.book_skips.contains_key(&book) || settings.book_skips.len() < 1024 {
        *settings.book_skips.entry(book).or_default() |= kind.bit();
    }
}

#[cfg(test)]
mod book_skip_tests {
    use super::*;

    #[test]
    fn only_selected_book_and_kinds_are_remembered() {
        let mut settings = ReadAloudSettings::default();
        let first = BookId(1);
        let second = BookId(2);
        remember_skip(&mut settings, first, parts::Kind::Table);
        remember_skip(&mut settings, first, parts::Kind::List);
        assert_eq!(settings.book_skips.get(&first), Some(&3));
        assert!(!settings.book_skips.contains_key(&second));
        assert_eq!(settings.skips, mareader_settings::Skips::default());
    }
}
