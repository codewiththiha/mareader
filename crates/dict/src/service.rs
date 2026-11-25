//! What the app holds: pack states, download threads, a lookup
//! worker.

use crate::ai::Saved;
use crate::entry::Entry;
use crate::packs::{PACKS, Pack};
use crate::route::Pair;

/// Where a pack stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Absent,
    Fetching { got: u64, total: Option<u64> },
    Paused { got: u64, total: Option<u64> },
    Converting,
    Ready,
    Failed(String),
}

/// Names one ask, to match its reply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ticket(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Query {
    /// A card's lookup, answered in `to` or the first other language.
    LookUp { word: String, to: Option<String> },
    /// A word list's ask: English words' rows in each of `to`.
    Words {
        words: Vec<String>,
        to: Vec<String>,
        near: bool,
    },
    /// The panel's search: between a pair, else in one pack or all.
    Search {
        text: String,
        pair: Option<Pair>,
        pack: Option<String>,
    },
    /// Keeps AI meanings; one already under its key stays unless `replace`.
    Remember { saved: Vec<Saved>, replace: bool },
    /// The AI meanings kept for `words` in `lang`, every class.
    Remembered { words: Vec<String>, lang: String },
    /// Drops an AI meaning, found by its key.
    Forget { saved: Box<Saved> },
    /// Writes `new` over `old`; a meaning under the new key stays unless
    /// `replace`, and the reply's `saved` names it.
    Edit {
        old: Box<Saved>,
        new: Box<Saved>,
        replace: bool,
    },
}

impl Query {
    /// A newer ask in the same lane drops an older one; none answers all.
    fn lane(&self) -> Option<u8> {
        match self {
            Self::LookUp { .. } => Some(0),
            Self::Search { .. } => Some(1),
            Self::Words { .. }
            | Self::Remember { .. }
            | Self::Remembered { .. }
            | Self::Forget { .. }
            | Self::Edit { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub ticket: Ticket,
    pub entries: Vec<Entry>,
    /// The languages asked between, when a route was taken.
    pub from: Option<String>,
    pub to: Option<String>,
    /// How many dictionaries were ready to ask, the AI one included.
    pub ready: usize,
    /// A pack that couldn't be read, in its own words.
    pub failed: Option<String>,
    /// A word list's answers, a word and language at a time.
    pub found: Vec<Found>,
    /// AI meanings: those kept, or those a `Remember` left in place.
    pub saved: Vec<Saved>,
    /// The AI dictionary's languages, when this ask read them.
    pub ai_langs: Option<Vec<String>>,
}

/// One word's rows in one language, for a word list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub word: String,
    pub to: String,
    /// Rows for the word itself.
    pub own: Vec<Entry>,
    /// Rows for headwords near it, as `Store::near` finds them.
    pub near: Vec<Entry>,
}

impl Reply {
    pub(crate) fn new(ready: usize) -> Self {
        Self {
            ticket: Ticket(0),
            entries: Vec::new(),
            from: None,
            to: None,
            ready,
            failed: None,
            found: Vec::new(),
            saved: Vec::new(),
            ai_langs: None,
        }
    }
}

/// The dictionaries: none until attached to a folder on a desktop.
#[derive(Default)]
pub struct Dictionaries {
    states: Vec<State>,
    next: u64,
    #[cfg(not(target_arch = "wasm32"))]
    desk: Option<desk::Desk>,
    /// Replies made on the caller's thread, as tests and browsers need.
    done: Vec<Reply>,
    /// The languages the AI dictionary keeps meanings in.
    ai_langs: Vec<String>,
    /// Asks from now on put the AI dictionary's rows first.
    ai_first: bool,
}

impl Dictionaries {
    #[cfg(not(target_arch = "wasm32"))]
    mareader_fetch::attached_method!();

    #[cfg(target_arch = "wasm32")]
    pub fn attached(&self) -> bool {
        false
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_inline(&mut self, _inline: bool) {}

    pub fn state(&self, pack: &Pack) -> &State {
        let i = PACKS.iter().position(|p| p.id == pack.id);
        i.and_then(|i| self.states.get(i)).unwrap_or(&State::Absent)
    }

    /// The packs ready to ask, in offer order.
    pub fn ready(&self) -> Vec<&'static Pack> {
        PACKS
            .iter()
            .filter(|p| *self.state(p) == State::Ready)
            .collect()
    }

    fn set(&mut self, pack: &Pack, state: State) {
        if self.states.len() < PACKS.len() {
            self.states.resize(PACKS.len(), State::Absent);
        }
        if let Some(i) = PACKS.iter().position(|p| p.id == pack.id)
            && let Some(slot) = self.states.get_mut(i)
        {
            *slot = state;
        }
    }

    /// Puts the AI dictionary's rows before the packs' in asks to come.
    pub fn set_ai_first(&mut self, ai_first: bool) {
        self.ai_first = ai_first;
    }

    /// Sends `query`; its reply comes from `poll`.
    pub fn ask(&mut self, query: Query) -> Ticket {
        self.next += 1;
        let ticket = Ticket(self.next);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(desk) = &mut self.desk {
            desk.ask(ticket, query, self.ai_first);
            return ticket;
        }
        let _ = (query.lane(), self.ai_first);
        let mut reply = Reply::new(0);
        reply.ticket = ticket;
        self.done.push(reply);
        ticket
    }

    /// The languages the AI dictionary on this computer keeps meanings in.
    pub fn ai_languages(&self) -> &[String] {
        &self.ai_langs
    }

    /// Takes in progress and replies; true if a pack's state changed.
    pub fn poll(&mut self) -> (bool, Vec<Reply>) {
        let mut replies = std::mem::take(&mut self.done);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(desk) = &mut self.desk {
            let (changes, more) = desk.poll();
            replies.extend(more);
            let changed = !changes.is_empty();
            for (pack, state) in changes {
                self.set(pack, state);
            }
            self.note_langs(&replies);
            return (changed, replies);
        }
        (false, replies)
    }

    /// Keeps the newest AI dictionary languages a reply read.
    #[cfg(not(target_arch = "wasm32"))]
    fn note_langs(&mut self, replies: &[Reply]) {
        let mut read = replies.iter().filter_map(|r| r.ai_langs.as_ref());
        if let Some(langs) = read.next_back() {
            self.ai_langs.clone_from(langs);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use desk::Waker;

#[cfg(not(target_arch = "wasm32"))]
impl Dictionaries {
    mareader_fetch::inline_method!();

    /// Keeps packs in `dir`, made if missing; `waker` repaints the app.
    pub fn attach(&mut self, dir: std::path::PathBuf, waker: Waker) -> Result<(), String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        self.ai_langs = crate::ai_store::langs_in(&dir);
        let (desk, states) = desk::Desk::start(dir, waker);
        self.states = states;
        self.desk = Some(desk);
        Ok(())
    }

    /// Starts or resumes `pack`'s download.
    pub fn download(&mut self, pack: &'static Pack) {
        let (got, total) = match self.state(pack) {
            State::Paused { got, total } => (*got, *total),
            State::Absent | State::Failed(_) => (0, None),
            _ => return,
        };
        if let Some(desk) = &mut self.desk {
            desk.download(pack);
            self.set(pack, State::Fetching { got, total });
        }
    }

    /// Stops a download, keeping what came to resume later.
    pub fn pause(&mut self, pack: &'static Pack) {
        if let Some(desk) = &mut self.desk {
            desk.halt(pack, mareader_fetch::Halt::Pause);
        }
    }

    /// Stops a download and drops what came.
    pub fn cancel(&mut self, pack: &'static Pack) {
        let idle = matches!(self.state(pack), State::Paused { .. } | State::Failed(_));
        if let Some(desk) = &mut self.desk {
            desk.halt(pack, mareader_fetch::Halt::Cancel);
            if idle {
                desk.remove(pack);
            }
        }
        self.set(pack, State::Absent);
    }

    /// Deletes a pack's files; it can be downloaded again.
    pub fn remove(&mut self, pack: &'static Pack) {
        if let Some(desk) = &mut self.desk {
            desk.halt(pack, mareader_fetch::Halt::Cancel);
            desk.remove(pack);
        }
        self.set(pack, State::Absent);
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod desk;
