//! Engine-owned dictionary packs, queries, and word-list lookups.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use mareader_dict::{Dictionaries, PACKS, Pack, Query, Reply, Saved, State, Ticket};

use mareader_study::Pos;

use super::{Action, Edited};
use crate::word_list::Meanings;

/// Word list asks kept waiting at once.
const WORD_ASKS: usize = 4;
/// Class asks kept waiting at once: open word cards and a recall.
const CLASS_ASKS: usize = 4;
/// AI dictionary asks whose replies are kept; older ones drop.
const AI_ASKS: usize = 16;

/// Dictionary pack manager and lookup queue owned by `Session`.
#[derive(Default)]
pub struct Service {
    service: Dictionaries,
    /// Word lists' asks and, once in, their replies; oldest first.
    words: Vec<(Ticket, Option<Reply>)>,
    /// Single words' class asks, kept apart so they evict no list's.
    classes: Vec<(Ticket, Option<Reply>)>,
    /// The open word card's class ask, and the word it is for.
    card: Option<(Ticket, String)>,
    /// AI dictionary asks, kept from the reader's replies.
    ai: Vec<(Ticket, Option<Reply>)>,
    /// The AI dictionary edit or removal under way, by its ask.
    editing: Option<(Ticket, Edited)>,
    /// The ready packs as last polled, so a poll tells when they change.
    told: Vec<&'static str>,
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

    /// The languages a pack looks English words up in.
    pub fn pack_languages(&self) -> Vec<&'static str> {
        self.service
            .ready()
            .into_iter()
            .filter(|p| p.source == mareader_dict::HUB)
            .map(|p| p.target)
            .collect()
    }

    /// Every language English words can be looked up in: the packs',
    /// then those only the AI dictionary keeps.
    pub fn languages(&self) -> Vec<String> {
        let ready = self.service.ready();
        let all = mareader_dict::targets(&ready, self.ai_languages());
        all.into_iter().map(str::to_owned).collect()
    }

    /// Puts AI dictionary rows before the packs' in lookups to come.
    pub fn set_ai_first(&mut self, ai_first: bool) {
        self.service.set_ai_first(ai_first);
    }

    /// The languages this computer's AI dictionary keeps meanings in.
    pub fn ai_languages(&self) -> &[String] {
        self.service.ai_languages()
    }

    /// Submits a dictionary lookup query, returning its ticket.
    pub fn ask(&mut self, query: Query) -> Ticket {
        self.service.ask(query)
    }

    /// Runs a pack download action; returns true if `action` was handled.
    pub(crate) fn act_pack(&mut self, action: &Action) -> bool {
        if self.act_ai(action) {
            return true;
        }
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
            Action::CloseCard
            | Action::Pursue(_)
            | Action::Window(_)
            | Action::Forget(_)
            | Action::Clear(_)
            | Action::Edit { .. }
            | Action::DropEdit => {}
        }
        let _ = pack;
        handled
    }

    /// An AI entry's edit or removal, or a language's; true if one.
    fn act_ai(&mut self, action: &Action) -> bool {
        let query = match action {
            Action::Forget(saved) => Query::Forget {
                saved: saved.clone(),
            },
            Action::Clear(lang) => Query::Clear {
                lang: lang.to_owned(),
            },
            Action::Edit { old, new, replace } => Query::Edit {
                old: old.clone(),
                new: new.clone(),
                replace: *replace,
            },
            Action::DropEdit => {
                self.editing = None;
                return true;
            }
            _ => return false,
        };
        self.editing = Some((self.service.ask(query), Edited::Waiting));
        true
    }

    /// The AI entry edit under way: waiting, taken or failed.
    pub fn edited(&self) -> Option<&Edited> {
        self.editing.as_ref().map(|(_, e)| e)
    }

    /// Settles the edit `reply` answers; true when the AI dictionary changed.
    fn edit_done(&mut self, reply: Reply) -> bool {
        let state = match (reply.failed, reply.saved.into_iter().next()) {
            (Some(why), _) => Some(Edited::Failed(why)),
            (None, Some(kept)) => Some(Edited::Taken(Box::new(kept))),
            (None, None) => None,
        };
        let done = state.is_none();
        self.editing = state.map(|s| (reply.ticket, s));
        done
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

    /// Files meanings in the AI dictionary; `take_ai` hands the reply,
    /// naming kept meanings that differ.
    pub fn remember(&mut self, saved: Vec<Saved>, replace: bool) -> Ticket {
        self.ask_ai(Query::Remember { saved, replace })
    }

    /// Asks what the AI dictionary keeps for `words` in `lang`.
    pub fn recall(&mut self, words: Vec<String>, lang: String) -> Ticket {
        self.ask_ai(Query::Remembered { words, lang })
    }

    fn ask_ai(&mut self, query: Query) -> Ticket {
        let ticket = self.service.ask(query);
        if self.ai.len() >= AI_ASKS {
            self.ai.remove(0);
        }
        self.ai.push((ticket, None));
        ticket
    }

    pub fn take_ai(&mut self, ticket: Ticket) -> Option<Reply> {
        let at = self
            .ai
            .iter()
            .position(|(t, r)| *t == ticket && r.is_some())?;
        self.ai.remove(at).1
    }

    /// Asks for the class `word`'s rows name; none without a pack.
    pub fn ask_class(&mut self, word: &str) -> Option<Ticket> {
        let to = self.languages().into_iter().next()?;
        let words = vec![word.to_owned()];
        let ticket = self.service.ask(Query::Words {
            words,
            to: vec![to],
            near: false,
        });
        if self.classes.len() >= CLASS_ASKS {
            self.classes.remove(0);
        }
        self.classes.push((ticket, None));
        Some(ticket)
    }

    /// Asks the class of the open word card's `word`, over any before.
    pub fn ask_card_class(&mut self, word: &str) {
        self.card = self.ask_class(word).map(|t| (t, word.to_owned()));
    }

    /// The open card's word and the class its rows name, once in.
    pub fn take_card_class(&mut self) -> Option<(String, Option<Pos>)> {
        let ticket = self.card.as_ref()?.0;
        let class = self.take_class(ticket)?;
        Some((self.card.take()?.1, class))
    }

    /// The class `ticket`'s rows name, once in; `Some(None)` names none.
    pub fn take_class(&mut self, ticket: Ticket) -> Option<Option<Pos>> {
        let at = self
            .classes
            .iter()
            .position(|(t, r)| *t == ticket && r.is_some())?;
        let reply = self.classes.remove(at).1?;
        let rows = reply.found.iter().flat_map(|f| f.own.iter());
        Some(Pos::of_cells(rows.filter_map(|e| e.pos.as_deref())))
    }

    pub fn take_words(&mut self, ticket: Ticket) -> Option<Reply> {
        let at = self
            .words
            .iter()
            .position(|(t, r)| *t == ticket && r.is_some())?;
        self.words.remove(at).1
    }

    /// Polls replies and pack changes: `(moved, changed, ui_replies)`.
    ///
    /// `moved` is any pack's state moving, a download's progress included;
    /// `changed` is what the dictionaries answer changing: a pack came or
    /// went, or AI entries were edited or deleted.
    pub fn poll(&mut self) -> (bool, bool, Vec<Reply>) {
        let (moved, replies) = self.service.poll();
        let ready: Vec<&'static str> = self.service.ready().iter().map(|p| p.id).collect();
        let mut changed = ready != self.told;
        self.told = ready;
        let mut other = Vec::new();
        for reply in replies {
            let editing = self.editing.as_ref().map(|(t, _)| *t);
            if editing == Some(reply.ticket) {
                changed |= self.edit_done(reply);
                continue;
            }
            let mut asks = (self.words.iter_mut())
                .chain(self.classes.iter_mut())
                .chain(self.ai.iter_mut());
            if let Some((_, slot)) = asks.find(|(t, _)| *t == reply.ticket) {
                *slot = Some(reply);
            } else {
                other.push(reply);
            }
        }
        (moved, changed, other)
    }
}

impl Meanings for Service {
    fn languages(&self) -> Vec<String> {
        Self::languages(self)
    }

    fn ask_words(&mut self, words: Vec<String>, to: Vec<String>, near: bool) -> Ticket {
        Self::ask_words(self, words, to, near)
    }

    fn take_words(&mut self, ticket: Ticket) -> Option<Reply> {
        Self::take_words(self, ticket)
    }

    fn remember(&mut self, saved: Vec<Saved>) {
        Self::remember(self, saved, false);
    }

    fn ask_kept(&mut self, words: Vec<String>, lang: String) -> Option<Ticket> {
        Some(self.recall(words, lang))
    }

    fn take_kept(&mut self, ticket: Ticket) -> Option<Reply> {
        self.take_ai(ticket)
    }
}
