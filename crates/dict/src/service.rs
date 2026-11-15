//! What the app holds: pack states, download threads, a lookup
//! worker.

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
}

impl Query {
    /// A newer ask in the same lane drops an older one; none answers all.
    fn lane(&self) -> Option<u8> {
        match self {
            Self::LookUp { .. } => Some(0),
            Self::Search { .. } => Some(1),
            Self::Words { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub ticket: Ticket,
    pub entries: Vec<Entry>,
    /// The languages asked between, when a route was taken.
    pub from: Option<String>,
    pub to: Option<String>,
    /// How many packs were ready to ask.
    pub ready: usize,
    /// A pack that couldn't be read, in its own words.
    pub failed: Option<String>,
    /// A word list's answers, a word and language at a time.
    pub found: Vec<Found>,
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

    /// Sends `query`; its reply comes from `poll`.
    pub fn ask(&mut self, query: Query) -> Ticket {
        self.next += 1;
        let ticket = Ticket(self.next);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(desk) = &mut self.desk {
            desk.ask(ticket, query);
            return ticket;
        }
        let _ = query.lane();
        let mut reply = Reply::new(0);
        reply.ticket = ticket;
        self.done.push(reply);
        ticket
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
            return (changed, replies);
        }
        (false, replies)
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
mod desk {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::time::{Duration, Instant};

    use super::{Query, Reply, State, Ticket};
    use crate::build::build;
    use crate::packs::{PACKS, Pack};
    use crate::shelf::{Shelf, building_file, db_file, part_file};
    use crate::store;
    use mareader_fetch::{self as fetch, Halt, Kind, Stopped};

    pub type Waker = Arc<dyn Fn() + Send + Sync>;

    /// Progress is told this often at most.
    const TELL_EVERY: Duration = Duration::from_millis(100);

    enum Job {
        Ask(Ticket, Query),
        Remove(&'static Pack),
    }

    enum Event {
        State(&'static Pack, State),
        Reply(Reply),
    }

    pub struct Desk {
        dir: PathBuf,
        jobs: Sender<Job>,
        events: Receiver<Event>,
        tell: Sender<Event>,
        halts: HashMap<&'static str, Arc<AtomicU8>>,
        waker: Waker,
        pub inline: bool,
        shelf: Option<Shelf>,
        replies: Vec<Reply>,
    }

    impl Desk {
        pub fn start(dir: PathBuf, waker: Waker) -> (Self, Vec<State>) {
            let (jobs, inbox) = channel();
            let (tell, events) = channel();
            let worker = (dir.clone(), tell.clone(), Arc::clone(&waker));
            std::thread::spawn(move || serve(worker.0, &inbox, &worker.1, &*worker.2));
            let mut desk = Self {
                dir,
                jobs,
                events,
                tell,
                halts: HashMap::new(),
                waker,
                inline: false,
                shelf: None,
                replies: Vec::new(),
            };
            let states = PACKS.iter().map(|p| desk.scan(p)).collect();
            (desk, states)
        }

        /// A pack's state on disk at start; a whole download converts.
        fn scan(&mut self, pack: &'static Pack) -> State {
            let _ = std::fs::remove_file(building_file(&self.dir, pack));
            if store::usable(&db_file(&self.dir, pack)) {
                return State::Ready;
            }
            let part = part_file(&self.dir, pack);
            match std::fs::metadata(&part).map(|m| m.len()) {
                Ok(_) if Kind::Parquet.check(&part).is_ok() => {
                    self.convert_later(pack);
                    State::Converting
                }
                Ok(got) => State::Paused { got, total: None },
                Err(_) => State::Absent,
            }
        }

        pub fn ask(&mut self, ticket: Ticket, query: Query) {
            if self.inline {
                let dir = self.dir.clone();
                let shelf = self.shelf.get_or_insert_with(|| Shelf::new(dir));
                let mut reply = shelf.answer(&query);
                reply.ticket = ticket;
                self.replies.push(reply);
                return;
            }
            let _ = self.jobs.send(Job::Ask(ticket, query));
        }

        pub fn remove(&mut self, pack: &'static Pack) {
            if let Some(shelf) = &mut self.shelf {
                shelf.forget(pack);
            }
            let _ = self.jobs.send(Job::Remove(pack));
        }

        pub fn halt(&mut self, pack: &Pack, halt: Halt) {
            if let Some(flag) = self.halts.get(pack.id) {
                flag.store(halt as u8, Ordering::Relaxed);
            }
        }

        pub fn download(&mut self, pack: &'static Pack) {
            let flag = Arc::new(AtomicU8::new(Halt::Go as u8));
            self.halts.insert(pack.id, Arc::clone(&flag));
            let (dir, tell, waker) = (self.dir.clone(), self.tell.clone(), Arc::clone(&self.waker));
            std::thread::spawn(move || {
                let say = |s: State| {
                    let _ = tell.send(Event::State(pack, s));
                    waker();
                };
                let part = part_file(&dir, pack);
                let mut last = Instant::now() - TELL_EVERY;
                let mut progress = |got, total| {
                    if last.elapsed() >= TELL_EVERY {
                        last = Instant::now();
                        say(State::Fetching { got, total });
                    }
                };
                match fetch::fetch(&pack.urls(), &part, Kind::Parquet, &flag, &mut progress) {
                    Ok(()) => {
                        say(State::Converting);
                        say(convert(&dir, pack));
                    }
                    Err(Stopped::Halted(Halt::Pause)) => {
                        let got = std::fs::metadata(&part).map_or(0, |m| m.len());
                        say(State::Paused { got, total: None });
                    }
                    Err(Stopped::Halted(_)) => say(State::Absent),
                    Err(Stopped::Failed(why)) => say(State::Failed(why)),
                }
            });
        }

        fn convert_later(&self, pack: &'static Pack) {
            let (dir, tell, waker) = (self.dir.clone(), self.tell.clone(), Arc::clone(&self.waker));
            std::thread::spawn(move || {
                let _ = tell.send(Event::State(pack, convert(&dir, pack)));
                waker();
            });
        }

        pub fn poll(&mut self) -> (Vec<(&'static Pack, State)>, Vec<Reply>) {
            let mut changes = Vec::new();
            let mut replies = std::mem::take(&mut self.replies);
            while let Ok(event) = self.events.try_recv() {
                match event {
                    Event::State(pack, state) => {
                        if !matches!(state, State::Fetching { .. }) {
                            self.halts.remove(pack.id);
                        }
                        changes.push((pack, state));
                    }
                    Event::Reply(reply) => replies.push(reply),
                }
            }
            (changes, replies)
        }
    }

    /// The whole download becomes the pack's table, then goes.
    fn convert(dir: &Path, pack: &Pack) -> State {
        let (part, building) = (part_file(dir, pack), building_file(dir, pack));
        let built = build(&part, &building).and_then(|_| {
            std::fs::rename(&building, db_file(dir, pack)).map_err(|e| e.to_string())
        });
        match built {
            Ok(()) => {
                let _ = std::fs::remove_file(&part);
                State::Ready
            }
            Err(why) => {
                let _ = std::fs::remove_file(&building);
                let _ = std::fs::remove_file(&part);
                State::Failed(format!("couldn't convert it: {why}"))
            }
        }
    }

    /// The lookup worker owns the open packs and skips superseded asks.
    fn serve(
        dir: PathBuf,
        inbox: &Receiver<Job>,
        tell: &Sender<Event>,
        waker: &(dyn Fn() + Send + Sync),
    ) {
        let mut shelf = Shelf::new(dir.clone());
        while let Ok(job) = inbox.recv() {
            let mut jobs = vec![job];
            jobs.extend(inbox.try_iter());
            let lanes: Vec<Option<u8>> = jobs
                .iter()
                .map(|j| match j {
                    Job::Ask(_, q) => q.lane(),
                    Job::Remove(_) => None,
                })
                .collect();
            for (i, job) in jobs.into_iter().enumerate() {
                match job {
                    Job::Ask(ticket, query) => {
                        let lane = query.lane();
                        if lane.is_some() && lanes.iter().skip(i + 1).any(|l| *l == lane) {
                            continue;
                        }
                        let mut reply = shelf.answer(&query);
                        reply.ticket = ticket;
                        let _ = tell.send(Event::Reply(reply));
                    }
                    Job::Remove(pack) => {
                        shelf.forget(pack);
                        for f in [
                            db_file(&dir, pack),
                            part_file(&dir, pack),
                            building_file(&dir, pack),
                        ] {
                            let _ = std::fs::remove_file(f);
                        }
                    }
                }
            }
            waker();
        }
    }
}
