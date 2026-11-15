//! Notices open files saved elsewhere, off the UI thread.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

/// How often open files are looked at.
pub const EVERY: Duration = Duration::from_secs(1);

/// A file's size and modified time: what changes when it is saved.
pub type Stamp = (u64, Option<SystemTime>);

/// What a file was last seen as, and a change not yet settled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    base: Option<Stamp>,
    pending: Option<Stamp>,
}

impl Seen {
    pub fn new(now: Option<Stamp>) -> Self {
        Self {
            base: now,
            pending: None,
        }
    }

    /// True once a change holds for two looks: never a half-written save.
    pub fn step(&mut self, now: Option<Stamp>) -> bool {
        match now {
            // Gone or unreadable for now: keep what is shown.
            None => self.pending = None,
            Some(s) if Some(s) == self.base => self.pending = None,
            Some(s) if Some(s) == self.pending => {
                (self.base, self.pending) = (Some(s), None);
                return true;
            }
            Some(s) => self.pending = Some(s),
        }
        false
    }
}

pub fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

/// What the looking thread is told.
#[derive(Default)]
struct State {
    paths: Vec<PathBuf>,
    /// Paths came or went: new ones are seen at once, as they open.
    fresh: bool,
    closed: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    told: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub struct Watch {
    shared: Arc<Shared>,
    changed: Receiver<PathBuf>,
    paths: Vec<PathBuf>,
}

impl Watch {
    /// Starts looking, calling `wake` on a change; none without threads.
    pub fn new(wake: impl Fn() + Send + 'static) -> Option<Self> {
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        let shared = Arc::new(Shared::default());
        let (tell, changed) = mpsc::channel();
        let looking = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("file watch".into())
            .spawn(move || run(&looking, &tell, &wake))
            .ok()?;
        Some(Self {
            shared,
            changed,
            paths: Vec::new(),
        })
    }

    /// Looks at exactly `paths` from now on.
    pub fn follow(&mut self, mut paths: Vec<PathBuf>) {
        paths.sort();
        paths.dedup();
        if paths != self.paths {
            let mut state = self.shared.lock();
            state.paths.clone_from(&paths);
            state.fresh = true;
            self.paths = paths;
            self.shared.told.notify_one();
        }
    }

    /// The files that changed since last asked.
    pub fn changed(&self) -> Vec<PathBuf> {
        self.changed.try_iter().collect()
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.shared.lock().closed = true;
        self.shared.told.notify_one();
    }
}

/// Looks every `EVERY` while there is a file to look at; sleeps otherwise.
fn run(shared: &Shared, tell: &Sender<PathBuf>, wake: &dyn Fn()) {
    let mut seen: HashMap<PathBuf, Seen> = HashMap::new();
    let mut next = Instant::now() + EVERY;
    loop {
        let paths = {
            let mut state = shared.lock();
            loop {
                if state.closed {
                    return;
                }
                if std::mem::take(&mut state.fresh) {
                    break;
                }
                let now = Instant::now();
                if state.paths.is_empty() {
                    state = shared
                        .told
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                } else if now >= next {
                    break;
                } else {
                    let waited = shared.told.wait_timeout(state, next - now);
                    state = waited.unwrap_or_else(PoisonError::into_inner).0;
                }
            }
            state.paths.clone()
        };
        seen.retain(|p, _| paths.contains(p));
        let tick = Instant::now() >= next;
        if tick {
            next = Instant::now() + EVERY;
        }
        let mut woke = false;
        for path in paths {
            let now = stamp(&path);
            let Some(file) = seen.get_mut(&path) else {
                seen.insert(path, Seen::new(now));
                continue;
            };
            if tick && file.step(now) {
                if tell.send(path).is_err() {
                    return;
                }
                woke = true;
            }
        }
        if woke {
            wake();
        }
    }
}

/// Files reopening after a save, and whether another save came meanwhile.
#[derive(Debug)]
pub struct Reloads<T, D>(Vec<Reload<T, D>>);

#[derive(Debug)]
struct Reload<T, D> {
    ticket: T,
    doc: D,
    path: PathBuf,
    again: bool,
}

impl<T, D> Default for Reloads<T, D> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T: PartialEq, D: PartialEq + Copy> Reloads<T, D> {
    pub fn has(&self, ticket: &T) -> bool {
        self.0.iter().any(|r| r.ticket == *ticket)
    }

    /// False if `doc` is opening again already; it then goes once more.
    pub fn should_start(&mut self, doc: D) -> bool {
        match self.0.iter_mut().find(|r| r.doc == doc) {
            Some(r) => {
                r.again = true;
                false
            }
            None => true,
        }
    }

    pub fn started(&mut self, ticket: T, doc: D, path: PathBuf) {
        self.0.push(Reload {
            ticket,
            doc,
            path,
            again: false,
        });
    }

    /// The document `ticket` reopened, its path, and whether to go again.
    pub fn finish(&mut self, ticket: &T) -> Option<(D, PathBuf, bool)> {
        let at = self.0.iter().position(|r| r.ticket == *ticket)?;
        let r = self.0.swap_remove(at);
        Some((r.doc, r.path, r.again))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_counts_once_it_has_held_still() {
        let t = |s| Some((s, None));
        let mut seen = Seen::new(t(10));
        assert!(!seen.step(t(10)), "unchanged");
        assert!(!seen.step(t(20)), "still being written");
        assert!(!seen.step(t(30)), "still being written");
        assert!(seen.step(t(30)), "settled");
        assert!(!seen.step(t(30)), "told once");
        // A file briefly gone, as an editor's save swaps it, is no change.
        assert!(!seen.step(None));
        assert!(!seen.step(t(30)));
        // One that appears counts as a change.
        let mut fresh = Seen::new(None);
        assert!(!fresh.step(t(5)));
        assert!(fresh.step(t(5)));
    }

    #[test]
    fn a_save_while_reopening_reopens_once_more() {
        let mut r: Reloads<u8, u8> = Reloads::default();
        assert!(r.should_start(7));
        r.started(1, 7, PathBuf::from("a.md"));
        assert!(r.has(&1));
        // Saved again before the first reopen is ready.
        assert!(!r.should_start(7));
        assert!(!r.should_start(7));
        assert_eq!(r.finish(&1), Some((7, PathBuf::from("a.md"), true)));
        assert!(!r.has(&1) && r.finish(&1).is_none());
        assert!(r.should_start(7));
        r.started(2, 7, PathBuf::from("a.md"));
        assert_eq!(r.finish(&2), Some((7, PathBuf::from("a.md"), false)));
    }
}
