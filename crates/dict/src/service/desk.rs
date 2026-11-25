//! The lookup worker and the download threads behind `Dictionaries`.

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
    /// An ask, and whether the AI dictionary's rows lead its reply.
    Ask(Ticket, Query, bool),
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

    pub fn ask(&mut self, ticket: Ticket, query: Query, ai_first: bool) {
        if self.inline {
            let dir = self.dir.clone();
            let shelf = self.shelf.get_or_insert_with(|| Shelf::new(dir));
            let mut reply = shelf.answer(&query, ai_first);
            reply.ticket = ticket;
            self.replies.push(reply);
            return;
        }
        let _ = self.jobs.send(Job::Ask(ticket, query, ai_first));
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
    let built = build(&part, &building)
        .and_then(|_| std::fs::rename(&building, db_file(dir, pack)).map_err(|e| e.to_string()));
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
                Job::Ask(_, q, _) => q.lane(),
                Job::Remove(_) => None,
            })
            .collect();
        for (i, job) in jobs.into_iter().enumerate() {
            match job {
                Job::Ask(ticket, query, ai_first) => {
                    let lane = query.lane();
                    if lane.is_some() && lanes.iter().skip(i + 1).any(|l| *l == lane) {
                        continue;
                    }
                    let mut reply = shelf.answer(&query, ai_first);
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
