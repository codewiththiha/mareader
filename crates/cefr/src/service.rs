//! What the app holds: download states, answered words, two workers.

use std::collections::HashSet;

use crate::levels::{Levels, band, plan_chars};
use crate::roles::Role;
use crate::text::Span;

/// Where a download stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Absent,
    Fetching {
        got: u64,
        total: Option<u64>,
    },
    Paused {
        got: u64,
        total: Option<u64>,
    },
    /// The dataset becoming a table; the tagger never is.
    Converting,
    Ready,
    Failed(String),
}

/// The two downloads: the word levels, and the tagger for roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Dataset,
    Tagger,
}

/// Names one role ask, to match its reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ticket(pub u64);

/// What the workers sent since the last poll.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polled {
    /// A download's state or the answered words changed.
    pub changed: bool,
    pub roles: Vec<(Ticket, Option<Role>)>,
}

/// What a worker sends back.
#[derive(Debug)]
enum Event {
    State(Part, State),
    Words(u64),
    Levels(Vec<(String, Option<f64>)>),
    /// The table couldn't answer these keys; why.
    Failed(Vec<String>, String),
    Role(Ticket, Option<Role>),
}

/// The vocabulary: nothing is hard until attached on a desktop.
#[derive(Default)]
pub struct Vocabulary {
    dataset: State,
    tagger: State,
    words: Option<u64>,
    levels: Levels,
    /// Keys sent to the worker and not yet answered.
    asked: HashSet<String>,
    generation: u64,
    next: u64,
    #[cfg(not(target_arch = "wasm32"))]
    desk: Option<desk::Desk>,
}

impl Vocabulary {
    #[cfg(not(target_arch = "wasm32"))]
    mareader_fetch::attached_method!();

    #[cfg(target_arch = "wasm32")]
    pub fn attached(&self) -> bool {
        false
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_inline(&mut self, _inline: bool) {}

    pub fn state(&self, part: Part) -> &State {
        match part {
            Part::Dataset => &self.dataset,
            Part::Tagger => &self.tagger,
        }
    }

    /// How many words the dataset knows, once ready.
    pub fn words(&self) -> Option<u64> {
        self.words
    }

    /// Raised whenever answers land: shaded text is planned again.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Up to `cap` words of `text` above `level`; unknown ones get asked.
    pub fn plan(&mut self, text: &str, level: u8, cap: usize) -> Vec<Span> {
        self.plan_chars(&text.chars().collect::<Vec<char>>(), level, cap)
    }

    /// As [`Self::plan`], over characters a caller already holds.
    pub fn plan_chars(&mut self, chars: &[char], level: u8, cap: usize) -> Vec<Span> {
        self.plan_banded_chars(chars, level, cap)
            .into_iter()
            .map(|(span, _)| span)
            .collect()
    }

    /// Hard words with their highest known CEFR band, in text order.
    pub fn plan_banded_chars(&mut self, chars: &[char], level: u8, cap: usize) -> Vec<(Span, u8)> {
        let planned = plan_chars(chars, &self.levels, level, cap);
        if self.dataset == State::Ready {
            let fresh: Vec<String> = planned
                .misses
                .into_iter()
                .filter(|k| self.asked.insert(k.clone()))
                .collect();
            #[cfg(not(target_arch = "wasm32"))]
            if let (false, Some(desk)) = (fresh.is_empty(), &mut self.desk) {
                desk.levels(fresh);
            }
            #[cfg(target_arch = "wasm32")]
            let _ = fresh;
        }
        planned.hard.into_iter().zip(planned.bands).collect()
    }

    /// Each key's band where known; unknown keys are asked once.
    pub fn bands(&mut self, keys: &[String]) -> Vec<Option<u8>> {
        let known: Vec<Option<u8>> = keys.iter().map(|k| self.levels.get(k)).collect();
        if self.dataset == State::Ready {
            let fresh: Vec<String> = (keys.iter().zip(&known))
                .filter(|(k, b)| b.is_none() && self.asked.insert((*k).clone()))
                .map(|(k, _)| k.clone())
                .collect();
            #[cfg(not(target_arch = "wasm32"))]
            if let (false, Some(desk)) = (fresh.is_empty(), &mut self.desk) {
                desk.levels(fresh);
            }
            #[cfg(target_arch = "wasm32")]
            let _ = fresh;
        }
        known
    }

    /// Whether keys still wait on the worker.
    pub fn asking(&self) -> bool {
        !self.asked.is_empty()
    }

    /// Asks for `word`'s role in `sentence`; none without the tagger.
    pub fn role(&mut self, word: &str, sentence: &str) -> Option<Ticket> {
        if self.tagger != State::Ready {
            return None;
        }
        self.next += 1;
        let ticket = Ticket(self.next);
        #[cfg(not(target_arch = "wasm32"))]
        self.desk.as_mut()?.role(ticket, word, sentence);
        #[cfg(target_arch = "wasm32")]
        let _ = (word, sentence);
        Some(ticket)
    }

    /// Takes in what the workers sent.
    pub fn poll(&mut self) -> Polled {
        let mut out = Polled::default();
        #[cfg(not(target_arch = "wasm32"))]
        let events = self.desk.as_mut().map(desk::Desk::poll).unwrap_or_default();
        #[cfg(target_arch = "wasm32")]
        let events: Vec<Event> = Vec::new();
        for event in events {
            out.changed = true;
            match event {
                Event::State(part, state) => self.set(part, state),
                Event::Words(n) => self.words = Some(n),
                Event::Levels(answers) => {
                    for (key, level) in answers {
                        self.asked.remove(&key);
                        self.levels.insert(&key, band(level));
                    }
                    self.generation += 1;
                }
                Event::Failed(keys, why) => {
                    for key in keys {
                        self.asked.remove(&key);
                    }
                    // A table that can't answer stops being asked.
                    self.set(Part::Dataset, State::Failed(why));
                }
                Event::Role(ticket, role) => out.roles.push((ticket, role)),
            }
        }
        out
    }

    fn set(&mut self, part: Part, state: State) {
        if part == Part::Tagger {
            self.tagger = state;
            return;
        }
        let was = self.dataset == State::Ready;
        let now = state == State::Ready;
        self.dataset = state;
        if was != now {
            // What was known, or not, is asked again.
            self.levels.clear();
            self.asked.clear();
            self.generation += 1;
            if !now {
                self.words = None;
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use desk::Waker;

#[cfg(not(target_arch = "wasm32"))]
impl Vocabulary {
    mareader_fetch::inline_method!();

    /// Keeps the files in `dir`, made if missing; `waker` repaints.
    pub fn attach(&mut self, dir: std::path::PathBuf, waker: Waker) -> Result<(), String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let (desk, (dataset, words, tagger)) = desk::Desk::start(dir, waker);
        self.words = words;
        self.set(Part::Dataset, dataset);
        self.set(Part::Tagger, tagger);
        self.desk = Some(desk);
        Ok(())
    }

    /// Starts or resumes whichever download isn't done or running.
    pub fn download(&mut self) {
        for part in [Part::Dataset, Part::Tagger] {
            let (got, total) = match self.state(part) {
                State::Paused { got, total } => (*got, *total),
                State::Absent | State::Failed(_) => (0, None),
                _ => continue,
            };
            if let Some(desk) = &mut self.desk {
                desk.download(part);
                self.set(part, State::Fetching { got, total });
            }
        }
    }

    /// Stops both downloads, keeping what came to resume later.
    pub fn pause(&mut self) {
        if let Some(desk) = &mut self.desk {
            desk.halt(mareader_fetch::Halt::Pause);
        }
    }

    /// Stops both downloads and drops what came; ready files stay.
    pub fn cancel(&mut self) {
        let Some(desk) = &mut self.desk else { return };
        desk.halt(mareader_fetch::Halt::Cancel);
        for part in [Part::Dataset, Part::Tagger] {
            let state = self.state(part).clone();
            if let (State::Paused { .. } | State::Failed(_), Some(desk)) = (&state, &mut self.desk)
            {
                desk.remove(part);
            }
            if state != State::Ready {
                self.set(part, State::Absent);
            }
        }
    }

    /// Deletes both files; they can be downloaded again.
    pub fn remove(&mut self) {
        if let Some(desk) = &mut self.desk {
            desk.halt(mareader_fetch::Halt::Cancel);
            desk.remove(Part::Dataset);
            desk.remove(Part::Tagger);
        }
        self.set(Part::Dataset, State::Absent);
        self.set(Part::Tagger, State::Absent);
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

    use mareader_fetch::{Halt, Kind, Stopped, fetch};

    use super::{Event, Part, State, Ticket};
    use crate::roles::Roles;
    use crate::source::{DATASET, Source, TAGGER};
    use crate::store::{Store, build, usable};

    pub type Waker = Arc<dyn Fn() + Send + Sync>;

    /// Progress is told this often at most.
    const TELL_EVERY: Duration = Duration::from_millis(100);
    const TABLE: &str = "cefr.db";

    enum Job {
        Levels(Vec<String>),
        Role(Ticket, String, String),
        /// Close the files, then delete this part's.
        Remove(Part),
    }

    fn source(part: Part) -> Source {
        match part {
            Part::Dataset => DATASET,
            Part::Tagger => TAGGER,
        }
    }

    fn kind(part: Part) -> Kind {
        match part {
            Part::Dataset => Kind::Parquet,
            Part::Tagger => Kind::Zstd,
        }
    }

    fn part_file(dir: &Path, part: Part) -> PathBuf {
        dir.join(format!("{}.part", source(part).file))
    }

    fn building(dir: &Path) -> PathBuf {
        dir.join(format!("{TABLE}.building"))
    }

    /// Every file `part` leaves in `dir`.
    fn files(dir: &Path, part: Part) -> Vec<PathBuf> {
        match part {
            Part::Dataset => vec![dir.join(TABLE), building(dir), part_file(dir, part)],
            Part::Tagger => vec![dir.join(TAGGER.file), part_file(dir, part)],
        }
    }

    pub struct Desk {
        dir: PathBuf,
        levels: Sender<Job>,
        roles: Sender<Job>,
        events: Receiver<Event>,
        tell: Sender<Event>,
        halts: HashMap<&'static str, Arc<AtomicU8>>,
        waker: Waker,
        pub inline: bool,
        /// What inline asks read, opened on first use.
        store: Option<Store>,
        tagger: Option<Roles>,
        done: Vec<Event>,
    }

    type Found = (State, Option<u64>, State);

    impl Desk {
        pub fn start(dir: PathBuf, waker: Waker) -> (Self, Found) {
            let (levels, level_jobs) = channel();
            let (roles, role_jobs) = channel();
            let (tell, events) = channel();
            for (jobs, serve) in [
                (level_jobs, serve_levels as Serve),
                (role_jobs, serve_roles),
            ] {
                let (dir, tell, waker) = (dir.clone(), tell.clone(), Arc::clone(&waker));
                std::thread::spawn(move || serve(&dir, &jobs, &tell, &*waker));
            }
            let mut desk = Self {
                dir,
                levels,
                roles,
                events,
                tell,
                halts: HashMap::new(),
                waker,
                inline: false,
                store: None,
                tagger: None,
                done: Vec::new(),
            };
            let found = desk.scan();
            (desk, found)
        }

        /// The files on disk at start; a whole dataset download converts.
        fn scan(&mut self) -> Found {
            let dir = self.dir.clone();
            let _ = std::fs::remove_file(building(&dir));
            let part = part_file(&dir, Part::Dataset);
            let (dataset, words) = match usable(&dir.join(TABLE)) {
                Some(n) => (State::Ready, Some(n)),
                None => match std::fs::metadata(&part).map(|m| m.len()) {
                    Ok(_) if Kind::Parquet.check(&part).is_ok() => {
                        self.convert_later();
                        (State::Converting, None)
                    }
                    Ok(got) => (State::Paused { got, total: None }, None),
                    Err(_) => (State::Absent, None),
                },
            };
            let model = dir.join(TAGGER.file);
            let tagger = if Kind::Zstd.check(&model).is_ok() {
                State::Ready
            } else {
                // A cut-short model looks whole; only the server knows.
                match std::fs::metadata(part_file(&dir, Part::Tagger)) {
                    Ok(m) => State::Paused {
                        got: m.len(),
                        total: None,
                    },
                    Err(_) => State::Absent,
                }
            };
            (dataset, words, tagger)
        }

        pub fn levels(&mut self, keys: Vec<String>) {
            if !self.inline {
                let _ = self.levels.send(Job::Levels(keys));
                return;
            }
            if self.store.is_none() {
                self.store = Store::open(&self.dir.join(TABLE)).ok();
            }
            self.done.push(answer(self.store.as_ref(), keys));
        }

        pub fn role(&mut self, ticket: Ticket, word: &str, sentence: &str) {
            let (word, sentence) = (word.to_owned(), sentence.to_owned());
            if !self.inline {
                let _ = self.roles.send(Job::Role(ticket, word, sentence));
                return;
            }
            if self.tagger.is_none() {
                self.tagger = Roles::load(&self.dir.join(TAGGER.file)).ok();
            }
            let store = Store::open(&self.dir.join(TABLE)).ok();
            let role = self
                .tagger
                .as_ref()
                .and_then(|t| t.of(&word, &sentence, store.as_ref()));
            self.done.push(Event::Role(ticket, role));
        }

        pub fn remove(&mut self, part: Part) {
            match part {
                Part::Dataset => self.store = None,
                Part::Tagger => self.tagger = None,
            }
            let to = match part {
                Part::Dataset => &self.levels,
                Part::Tagger => &self.roles,
            };
            let _ = to.send(Job::Remove(part));
        }

        pub fn halt(&mut self, halt: Halt) {
            for flag in self.halts.values() {
                flag.store(halt as u8, Ordering::Relaxed);
            }
        }

        pub fn download(&mut self, part: Part) {
            let flag = Arc::new(AtomicU8::new(Halt::Go as u8));
            let src = source(part);
            self.halts.insert(src.file, Arc::clone(&flag));
            let (dir, tell, waker) = (self.dir.clone(), self.tell.clone(), Arc::clone(&self.waker));
            std::thread::spawn(move || {
                let say = |e: Event| {
                    let _ = tell.send(e);
                    waker();
                };
                let file = part_file(&dir, part);
                let mut last = Instant::now() - TELL_EVERY;
                let mut progress = |got, total| {
                    if last.elapsed() >= TELL_EVERY {
                        last = Instant::now();
                        say(Event::State(part, State::Fetching { got, total }));
                    }
                };
                let state = match fetch(&src.urls(), &file, kind(part), &flag, &mut progress) {
                    Ok(()) if part == Part::Dataset => {
                        say(Event::State(part, State::Converting));
                        let (state, words) = convert(&dir);
                        if let Some(n) = words {
                            say(Event::Words(n));
                        }
                        state
                    }
                    Ok(()) => match std::fs::rename(&file, dir.join(src.file)) {
                        Ok(()) => State::Ready,
                        Err(e) => State::Failed(e.to_string()),
                    },
                    Err(Stopped::Halted(Halt::Pause)) => {
                        let got = std::fs::metadata(&file).map_or(0, |m| m.len());
                        State::Paused { got, total: None }
                    }
                    Err(Stopped::Halted(_)) => State::Absent,
                    Err(Stopped::Failed(why)) => State::Failed(why),
                };
                say(Event::State(part, state));
            });
        }

        fn convert_later(&self) {
            let (dir, tell, waker) = (self.dir.clone(), self.tell.clone(), Arc::clone(&self.waker));
            std::thread::spawn(move || {
                let (state, words) = convert(&dir);
                if let Some(n) = words {
                    let _ = tell.send(Event::Words(n));
                }
                let _ = tell.send(Event::State(Part::Dataset, state));
                waker();
            });
        }

        pub fn poll(&mut self) -> Vec<Event> {
            let mut out = std::mem::take(&mut self.done);
            out.extend(self.events.try_iter());
            for event in &out {
                if let Event::State(part, state) = event
                    && !matches!(state, State::Fetching { .. })
                {
                    self.halts.remove(source(*part).file);
                }
            }
            out
        }
    }

    /// The whole download becomes the table, then goes.
    fn convert(dir: &Path) -> (State, Option<u64>) {
        let (part, build_to) = (part_file(dir, Part::Dataset), building(dir));
        let built = build(&part, &build_to)
            .and_then(|_| std::fs::rename(&build_to, dir.join(TABLE)).map_err(|e| e.to_string()));
        let _ = std::fs::remove_file(&part);
        match built.map(|()| usable(&dir.join(TABLE))) {
            Ok(Some(words)) => (State::Ready, Some(words)),
            Ok(None) => (State::Failed("the word table can't be read".into()), None),
            Err(why) => {
                let _ = std::fs::remove_file(&build_to);
                (State::Failed(format!("couldn't convert it: {why}")), None)
            }
        }
    }

    /// The levels of `keys`, or why the table couldn't say.
    fn answer(store: Option<&Store>, keys: Vec<String>) -> Event {
        let Some(store) = store else {
            return Event::Failed(keys, "the word table can't be opened".into());
        };
        match store.levels(&keys) {
            Ok(found) => {
                let answers = keys
                    .into_iter()
                    .map(|k| {
                        let level = found.get(&k).copied();
                        (k, level)
                    })
                    .collect();
                Event::Levels(answers)
            }
            Err(why) => Event::Failed(keys, why),
        }
    }

    type Serve = fn(&Path, &Receiver<Job>, &Sender<Event>, &(dyn Fn() + Send + Sync));

    /// Owns the open table; asks waiting together go as one.
    fn serve_levels(
        dir: &Path,
        jobs: &Receiver<Job>,
        tell: &Sender<Event>,
        waker: &(dyn Fn() + Send + Sync),
    ) {
        let mut store: Option<Store> = None;
        while let Ok(job) = jobs.recv() {
            let mut keys = Vec::new();
            for job in std::iter::once(job).chain(jobs.try_iter()) {
                match job {
                    Job::Levels(more) => keys.extend(more),
                    Job::Remove(part) => {
                        store = None;
                        for f in files(dir, part) {
                            let _ = std::fs::remove_file(f);
                        }
                    }
                    Job::Role(..) => {}
                }
            }
            if keys.is_empty() {
                continue;
            }
            if store.is_none() {
                store = Store::open(&dir.join(TABLE)).ok();
            }
            let _ = tell.send(answer(store.as_ref(), keys));
            waker();
        }
    }

    /// Owns the loaded tagger; a newer ask supersedes a waiting one.
    fn serve_roles(
        dir: &Path,
        jobs: &Receiver<Job>,
        tell: &Sender<Event>,
        waker: &(dyn Fn() + Send + Sync),
    ) {
        let mut tagger: Option<Roles> = None;
        while let Ok(job) = jobs.recv() {
            let mut latest = None;
            for job in std::iter::once(job).chain(jobs.try_iter()) {
                match job {
                    Job::Role(ticket, word, sentence) => {
                        if let Some((old, ..)) = latest.replace((ticket, word, sentence)) {
                            let _ = tell.send(Event::Role(old, None));
                        }
                    }
                    Job::Remove(part) => {
                        tagger = None;
                        for f in files(dir, part) {
                            let _ = std::fs::remove_file(f);
                        }
                    }
                    Job::Levels(_) => {}
                }
            }
            let Some((ticket, word, sentence)) = latest else {
                continue;
            };
            if tagger.is_none() {
                match Roles::load(&dir.join(TAGGER.file)) {
                    Ok(t) => tagger = Some(t),
                    Err(why) => {
                        let failed = State::Failed(format!("couldn't load it: {why}"));
                        let _ = tell.send(Event::State(Part::Tagger, failed));
                    }
                }
            }
            // Opened per ask, so only the levels worker holds the table.
            let store = Store::open(&dir.join(TABLE)).ok();
            let role = tagger
                .as_ref()
                .and_then(|t| t.of(&word, &sentence, store.as_ref()));
            let _ = tell.send(Event::Role(ticket, role));
            waker();
        }
    }
}
