//! One-call frame polling for every engine-owned background job.

use mareader_study::Pos;

use crate::event::Event;
use crate::library::Out;

use super::Session;

/// What a client should schedule after [`Session::tick`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tick {
    /// Engine state or job progress changed: draw another frame now.
    pub repaint: bool,
    /// Wake again after this many seconds (deferred save or debounce).
    pub wait: Option<f64>,
}

impl Session {
    /// Whether any engine-owned background job is still in flight.
    pub fn busy(&self) -> bool {
        self.documents.opening()
            || self.library.loading()
            || !self.library.runs.list().is_empty()
            || !self.library.folders.pending.is_empty()
            || self.search.busy()
            || self.word_lists.busy()
            || self.page_copy.busy()
            || self.vocab.coming()
            || self.study_mode.busy()
            || self.skills.running()
    }

    /// Advances every engine-owned background job and timer for time `now`.
    ///
    /// Idempotent when called more than once in the same frame. Results leave
    /// as [`Event`]s ready for [`Session::drain`].
    pub fn tick(&mut self, now: f64) -> Tick {
        match crate::guard::catch_panic(|| self.tick_inner(now)) {
            Ok(tick) => tick,
            Err(msg) => {
                self.emit_panic("tick", &msg);
                Tick::default()
            }
        }
    }

    fn tick_inner(&mut self, now: f64) -> Tick {
        self.reconcile_looks(now);
        self.reconcile_texts();
        let mut tick = Tick::default();
        self.tick_library(now, &mut tick);
        self.forget_closed_searches();
        if self.poll_search() {
            tick.repaint = true;
        }
        self.tick_clips();
        self.read_aloud_find();
        self.read_aloud_remember();
        if self.study_receive() {
            tick.repaint = true;
        }
        self.tick_vocab_and_audio(now, &mut tick);
        self.page_ai_receive(now);
        if self.skills_receive() {
            tick.repaint = true;
        }
        if let Some(wait) = self.page_ai.take_wait() {
            tick.wait = merge_wait(tick.wait, Some(wait.as_secs_f64()));
        }
        self.poll_page_copy();
        self.tick_dict_and_word_list(&mut tick);
        // Deck stages wait on time as well as answers, so they need frames.
        if self.tick_study_mode(now) {
            tick.repaint = true;
        }
        if self.poll_updates() {
            tick.repaint = true;
        }
        tick
    }

    fn tick_library(&mut self, now: f64, tick: &mut Tick) {
        for ask in self.library_receive(now) {
            match ask {
                Out::Toast(toast) => {
                    tick.repaint = true;
                    self.emit(Event::Toast(toast));
                }
                Out::Command(command) => {
                    if self.execute(command, now).dirty {
                        tick.repaint = true;
                    }
                }
            }
        }
        tick.wait = merge_wait(tick.wait, self.library_save(now));
    }

    fn tick_vocab_and_audio(&mut self, now: f64, tick: &mut Tick) {
        let (changed, role) = self.poll_vocab();
        if changed {
            tick.repaint = true;
        }
        if let Some((word, role)) = role {
            let named = role.as_ref().and_then(|r| Pos::of_tag(&r.pos));
            if named.is_none() {
                self.dict.ask_card_class(&word);
            }
            if let Some(role) = role {
                self.emit(Event::Role { word, role });
            }
        }
        if self.poll_voice_shelf() {
            tick.repaint = true;
        }
        if self.poll_sounds(now) {
            tick.repaint = true;
        }
    }

    fn tick_dict_and_word_list(&mut self, tick: &mut Tick) {
        self.dict.set_ai_first(self.settings.dictionary.ai_first);
        let (changed, mut replies) = self.dict_poll();
        if self.study_offers() {
            tick.repaint = true;
        }
        let ai_first = self.settings.dictionary.ai_first;
        for reply in &mut replies {
            crate::ai_dict::lead(reply, ai_first);
        }
        let edited = self.dict.take_edited();
        if changed || edited || !replies.is_empty() {
            tick.repaint = true;
            let replies = Event::DictReplies {
                changed,
                edited,
                replies,
            };
            self.emit(replies);
        }
        if let Some((word, Some(pos))) = self.dict.take_card_class() {
            tick.repaint = true;
            self.emit(Event::Class { word, pos });
        }
        if !self.making_word_list() {
            return;
        }
        // Answers land by waker, but a list's own steps need frames.
        tick.repaint = true;
        match self.poll_word_list() {
            None => {}
            Some(Err(toast)) => self.emit(Event::Toast(toast)),
            Some(Ok((made, list))) => self.emit(Event::WordList { made, list }),
        }
    }
}

fn merge_wait(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (a, b) => a.or(b),
    }
}
