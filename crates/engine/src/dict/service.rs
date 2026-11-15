//! Engine-owned dictionary packs, queries, and word-list lookups.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use mareader_dict::{Dictionaries, PACKS, Pack, Query, Reply, State, Ticket};

use super::Action;
use crate::word_list::Meanings;

/// Word list asks kept waiting at once.
const WORD_ASKS: usize = 4;

/// Dictionary pack manager and lookup queue owned by `Session`.
#[derive(Default)]
pub struct Service {
    service: Dictionaries,
    /// Word lists' asks and, once in, their replies; oldest first.
    words: Vec<(Ticket, Option<Reply>)>,
}

impl Service {
    /// Keeps packs in `dir`; `waker` repaints the client as work lands.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach(&mut self, dir: PathBuf, waker: mareader_dict::Waker) -> Result<(), String> {
        self.service.attach(dir, waker)
    }

    /// Answers on this thread, as tests need.
    pub fn set_inline(&mut self, inline: bool) {
        self.service.set_inline(inline);
    }

    pub fn attached(&self) -> bool {
        self.service.attached()
    }

    pub fn state(&self, pack: &Pack) -> &State {
        self.service.state(pack)
    }

    pub fn ready(&self) -> Vec<&'static Pack> {
        self.service.ready()
    }

    /// The languages English words can be looked up in.
    pub fn languages(&self) -> Vec<&'static str> {
        self.service
            .ready()
            .into_iter()
            .filter(|p| p.source == mareader_dict::HUB)
            .map(|p| p.target)
            .collect()
    }

    /// Submits a dictionary lookup query, returning its ticket.
    pub fn ask(&mut self, query: Query) -> Ticket {
        self.service.ask(query)
    }

    /// Runs a pack download action; returns true if `action` was handled.
    pub fn act_pack(&mut self, action: &Action) -> bool {
        let pack = |id: &str| PACKS.iter().find(|p| p.id == id);
        let handled = matches!(
            action,
            Action::Download(_) | Action::Pause(_) | Action::Cancel(_) | Action::Remove(_)
        );
        #[cfg(not(target_arch = "wasm32"))]
        match action {
            Action::Download(id) => {
                if let Some(p) = pack(id) {
                    self.service.download(p);
                }
            }
            Action::Pause(id) => {
                if let Some(p) = pack(id) {
                    self.service.pause(p);
                }
            }
            Action::Cancel(id) => {
                if let Some(p) = pack(id) {
                    self.service.cancel(p);
                }
            }
            Action::Remove(id) => {
                if let Some(p) = pack(id) {
                    self.service.remove(p);
                }
            }
            Action::CloseCard | Action::Pursue(_) | Action::Window(_) => {}
        }
        let _ = pack;
        handled
    }

    /// Asks for a word list's rows; `take_words` hands the reply over.
    pub fn ask_words(&mut self, words: Vec<String>, to: Vec<String>, near: bool) -> Ticket {
        let ticket = self.service.ask(Query::Words { words, to, near });
        // A list given up on leaves its ask; a few are kept at most.
        if self.words.len() >= WORD_ASKS {
            self.words.remove(0);
        }
        self.words.push((ticket, None));
        ticket
    }

    pub fn take_words(&mut self, ticket: Ticket) -> Option<Reply> {
        let at = self
            .words
            .iter()
            .position(|(t, r)| *t == ticket && r.is_some())?;
        self.words.remove(at).1
    }

    /// Polls replies and pack changes; returns `(changed, ui_replies)`.
    pub fn poll(&mut self) -> (bool, Vec<Reply>) {
        let (changed, replies) = self.service.poll();
        let mut other = Vec::new();
        for reply in replies {
            if let Some((_, slot)) = self.words.iter_mut().find(|(t, _)| *t == reply.ticket) {
                *slot = Some(reply);
            } else {
                other.push(reply);
            }
        }
        (changed, other)
    }
}

impl Meanings for Service {
    fn languages(&self) -> Vec<&'static str> {
        Self::languages(self)
    }

    fn ask_words(&mut self, words: Vec<String>, to: Vec<String>, near: bool) -> Ticket {
        Self::ask_words(self, words, to, near)
    }

    fn take_words(&mut self, ticket: Ticket) -> Option<Reply> {
        Self::take_words(self, ticket)
    }
}
