//! Each frame of a reading: what the voice said, and what it says next.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::TryRecvError;

use mareader_settings::{Pass, ReadAloudSettings};
use mareader_speech::{Event, Shelf, Spot};
use mareader_workspace::PaneId;

use super::{BEHIND, Spoken};
use crate::read_aloud::TICK;
use crate::read_aloud::feed::{Counts, Reach, Said, Start, enough, spawn};
use crate::read_aloud::place::{Panes, Stand, follow};
use crate::read_aloud::player::Act;

impl Spoken {
    /// Each frame: hears the voice, says on, turns `here`; false once done.
    pub fn tick(
        &mut self,
        panes: &mut dyn Panes,
        here: Option<&Stand>,
        (settings, shelf, clear): (&ReadAloudSettings, &Shelf, f32),
    ) -> bool {
        // Set in new type or opened again, the place is found anew.
        if let Some(reading) = here {
            self.doc = reading.doc;
            if !Arc::ptr_eq(&reading.document, &self.document) {
                self.document = Arc::clone(&reading.document);
                self.relay(reading.current);
            }
        }
        self.apply((settings, shelf));
        let events = self.voicer.as_mut().map(|v| v.events()).unwrap_or_default();
        for event in events {
            if event == Event::Finished(self.id) && self.saying {
                self.saying = false;
                let bytes = self.current.as_ref().map_or(0, |s| s.sentence.text.len());
                self.pace.learn(bytes, self.clock.seconds(), self.tone.rate);
                self.steady
                    .learn(bytes, self.clock.seconds(), self.tone.rate);
                self.pass();
            }
        }
        let heard = self.voicer.as_ref().and_then(|v| v.heard());
        self.heard = heard.filter(|h| h.id == self.id && self.saying);
        self.timed |= heard.is_some();
        if self.relaid {
            self.relocate();
        }
        if self.relaid {
            // Nothing more is said until the place is found.
            self.repaint.after(TICK);
            return true;
        }
        // Paused, a step still fetches the sentence it lands on.
        let fetch = self.current.is_none() && self.feed.is_some();
        if (!self.paused || fetch) && !self.saying && !self.finished {
            self.go_on();
        }
        if !self.finished {
            self.look_ahead();
        }
        if self.began.is_none() {
            self.began = self.current.as_ref().map(|s| s.sentence.start);
        }
        // Away from its pane, the book reads on unseen.
        let Some(reading) = here else {
            self.jump = None;
            if !self.paused && !self.finished {
                self.repaint.after(TICK);
            }
            return true;
        };
        // Back in a pane, it shows the words read at once.
        if std::mem::take(&mut self.homing) {
            self.jump = self.lit_spots().map(|(start, _)| start.page);
        }
        if let Some(page) = self.jump.take() {
            panes.go_to(self.pane, page);
            self.followed = Some(page);
        }
        let lit = self.lit_spots();
        let page = lit.map(|(start, _)| start.page);
        // Only a real turn shows the new page's first sentence.
        let turned = page.is_some() && page != self.followed && page != Some(reading.current);
        follow(panes, reading, settings, page, &mut self.followed);
        self.reveal(panes, (reading.pane, clear), settings, turned);
        if !self.paused && !self.finished {
            self.repaint.after(TICK);
        }
        true
    }

    /// New pages: the voice goes on while its place is found on them.
    fn relay(&mut self, page: usize) {
        // Pages laid anew hold new counts.
        self.counts = Counts::default();
        self.began = None;
        // A selection's end was on the old pages: its reading ends here.
        if self.chosen.is_some() {
            if let Some(v) = self.voicer.as_mut() {
                v.stop();
            }
            (self.saying, self.finished) = (false, true);
            (self.feed, self.current) = (None, None);
            self.ahead.clear();
            return;
        }
        let anchor = self.current.as_ref().and_then(|s| s.anchor);
        let (Some(anchor), Some(_)) = (anchor, self.voicer.as_ref()) else {
            self.read_from(Start::Spot(Spot { page, char: 0 }));
            return;
        };
        // Places behind and ahead are the old pages'; ahead comes again.
        self.behind.clear();
        self.ahead.clear();
        self.followed = None;
        self.revealed = None;
        self.relaid = true;
        let document = Arc::clone(&self.document);
        let to = (Arc::clone(&self.wake), Arc::clone(&self.counts));
        let span = Reach {
            start: Start::Anchor(anchor),
            until: None,
        };
        self.feed = Some(spawn(document, span, self.skip_folios, to));
    }

    /// The new pages' first sentence is the one said: it takes its place.
    fn relocate(&mut self) {
        let Some(found) = self.pull() else {
            // The book ended first: the old place has to do.
            self.relaid = self.feed.is_some();
            return;
        };
        self.relaid = false;
        // Said to its end meanwhile: it is behind, and reading goes on.
        let Some(said) = self.current.as_mut() else {
            return;
        };
        said.sentence.start = found.sentence.start;
        said.sentence.end = found.sentence.end;
        said.sentence.words = found.sentence.words;
        said.anchor = found.anchor;
    }

    /// Keeps the sentence said in view, above the player `clear` high.
    fn reveal(
        &mut self,
        panes: &mut dyn Panes,
        (pane, clear): (PaneId, f32),
        s: &ReadAloudSettings,
        turned: bool,
    ) {
        let Some((start, end)) = self.lit_spots() else {
            return;
        };
        // A turn lands first; the line is brought up once it has.
        if turned || !s.follow || self.revealed == Some(start) {
            return;
        }
        let last = if end.page == start.page {
            end.char
        } else {
            start.char + 1
        };
        if panes.reveal_said(pane, start.page, start.char..last, clear) {
            self.revealed = Some(start);
        }
    }

    /// Says the current sentence, fetching it first if there is none.
    pub(super) fn go_on(&mut self) {
        if self.current.is_none() {
            self.current = self.next();
        }
        match (self.current.is_some(), self.feed.is_some()) {
            (true, _) if !self.paused => self.say(),
            (true, _) => {}
            (false, true) => {}
            (false, false) => self.finished = true,
        }
    }

    /// The next sentence; none while the thread reads, the end once done.
    fn next(&mut self) -> Option<Said> {
        while let Some(said) = self.ahead.pop_front().or_else(|| self.pull()) {
            if self.leaving.is_some_and(|p| said.parts.contains(&p)) {
                self.keep(said);
                continue;
            }
            self.leaving = None;
            self.cleared.retain(|p| said.parts.contains(p));
            if std::mem::take(&mut self.entered) {
                self.cleared.clone_from(&said.parts);
                return Some(said);
            }
            // Outermost first: a skipped list passes the tables in it.
            let mut held = None;
            for &part in &said.parts {
                if self.cleared.contains(&part) {
                    continue;
                }
                match part.kind.pass(&self.skips) {
                    Pass::Read => self.cleared.push(part),
                    pass => {
                        held = Some((part, pass));
                        break;
                    }
                }
            }
            match held {
                Some((part, Pass::Skip)) => {
                    self.leaving = Some(part);
                    self.keep(said);
                }
                Some((part, _)) => {
                    self.ask(part);
                    return Some(said);
                }
                None => return Some(said),
            }
        }
        None
    }

    /// Hands the current sentence to the voice.
    pub(super) fn say(&mut self) {
        let (Some(v), Some(said)) = (self.voicer.as_mut(), self.current.as_ref()) else {
            return;
        };
        self.id = self.id.wrapping_add(1);
        self.heard = None;
        match v.say(self.id, &said.sentence.text) {
            Ok(()) => {
                self.saying = true;
                self.clock.start();
            }
            Err(fault) => {
                self.fault = Some(fault.to_string());
                self.saying = false;
                self.paused = true;
            }
        }
    }

    /// A new sentence off the pages, if the thread has one.
    fn pull(&mut self) -> Option<Said> {
        match self.feed.as_ref()?.try_recv() {
            Ok(said) => Some(said),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.feed = None;
                None
            }
        }
    }

    /// Hands the voice the sentences after the current, to make ahead.
    fn look_ahead(&mut self) {
        let texts = |ahead: &VecDeque<Said>| enough(ahead.iter().map(|s| s.sentence.text.as_str()));
        let mut wanted = texts(&self.ahead);
        while wanted.is_none()
            && let Some(said) = self.pull()
        {
            self.ahead.push_back(said);
            wanted = texts(&self.ahead);
        }
        let count = wanted.unwrap_or(self.ahead.len());
        let texts: Vec<&str> = (self.ahead.iter().take(count))
            .map(|s| s.sentence.text.as_str())
            .collect();
        if let Some(v) = self.voicer.as_mut() {
            v.prepare(&texts);
        }
    }

    /// Moves past the current sentence, keeping it to step back to.
    pub(super) fn pass(&mut self) {
        if let Some(said) = self.current.take() {
            self.keep(said);
        }
    }

    /// Keeps `said` to step back to, forgetting the oldest.
    fn keep(&mut self, said: Said) {
        self.behind.push_back(said);
        if self.behind.len() > BEHIND {
            self.behind.pop_front();
        }
    }

    /// Does what the player asked; true to stop.
    pub fn act(&mut self, act: Act) -> bool {
        match act {
            Act::Back => self.back(),
            Act::Next => self.forward(),
            Act::Toggle => self.toggle(),
            Act::Seek(share) if self.chosen.is_none() && self.voicer.is_some() => {
                let page = self.page_at(share);
                self.read_from(Start::Spot(Spot { page, char: 0 }));
                self.jump = Some(page);
            }
            Act::Escape => self.escape(),
            Act::Answer(read) => self.answer(read),
            Act::Seek(_) | Act::Card(_) | Act::Reveal => {}
            Act::Stop => return true,
        }
        false
    }

    pub fn playing(&self) -> bool {
        !self.paused && !self.finished && self.voicer.is_some()
    }

    pub fn pause(&mut self) {
        if self.paused || self.finished {
            return;
        }
        if let Some(v) = self.voicer.as_mut()
            && !v.pause()
        {
            v.stop();
            self.saying = false;
        }
        self.paused = true;
        self.clock.stop();
    }

    pub fn toggle(&mut self) {
        // Play, asked about a part, reads it.
        if self.asking.is_some() {
            self.answer(true);
        } else if self.finished {
            // Again from the selection's start, else the book's.
            self.paused = false;
            let start = self.chosen.map(|(start, _)| start);
            self.read_from(start.unwrap_or(Start::Spot(Spot::default())));
        } else if self.paused {
            self.paused = false;
            let resumed = self.saying && self.voicer.as_mut().is_some_and(|v| v.resume());
            match resumed {
                true => self.clock.go(),
                false => self.go_on(),
            }
        } else {
            self.pause();
        }
    }

    fn back(&mut self) {
        let Some(said) = self.behind.pop_back() else {
            return;
        };
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
        self.saying = false;
        self.finished = false;
        self.leaving = None;
        if self.asking.take().is_some() {
            self.paused = false;
        }
        // Stepped back into, its parts are read unasked.
        self.cleared.clone_from(&said.parts);
        if let Some(current) = self.current.replace(said) {
            self.ahead.push_front(current);
        }
        self.go_on();
    }

    /// Skips the rest of the innermost part read, to the sentence after it.
    fn escape(&mut self) {
        if self.asking.is_some() {
            self.answer(false);
            return;
        }
        let Some(&part) = self.current.as_ref().and_then(|s| s.parts.last()) else {
            return;
        };
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
        self.saying = false;
        self.leaving = Some(part);
        self.pass();
        self.go_on();
    }

    fn forward(&mut self) {
        // Stepped on by hand, the part asked about is read.
        if let Some(part) = self.asking.take() {
            self.cleared.push(part);
            self.paused = false;
        }
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
        self.saying = false;
        self.pass();
        self.go_on();
    }
}
