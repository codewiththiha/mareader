//! Headless engine runtime with condvar wake gate and background job settling.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use mareader_engine::command::Command;
use mareader_engine::event::Event;
use mareader_engine::host::{Host, Shared};
use mareader_engine::session::{Session, Waker};

const SETTLE_LIMIT: Duration = Duration::from_secs(30);
const WAIT_STEP: Duration = Duration::from_millis(10);

/// Minimal headless [`Host`] capturing clipboard writes and optional state dir.
pub struct McpHost {
    state: Option<PathBuf>,
    cache: Option<PathBuf>,
    clipboard: Mutex<Vec<String>>,
}

impl McpHost {
    /// Creates a headless host backed by optional `state` directory.
    pub fn new(state: Option<PathBuf>) -> Self {
        let cache = state.as_ref().map(|d| d.join("cache"));
        Self {
            state,
            cache,
            clipboard: Mutex::new(Vec::new()),
        }
    }

    /// Drains any clipboard text written during the most recent command.
    pub fn take_clipboard(&self) -> Vec<String> {
        let mut guard = lock(&self.clipboard);
        std::mem::take(&mut *guard)
    }
}

impl Host for McpHost {
    fn state_dir(&self) -> Option<PathBuf> {
        self.state.clone()
    }

    fn cache_dir(&self) -> Option<PathBuf> {
        self.cache.clone()
    }

    fn set_clipboard(&self, text: String) {
        lock(&self.clipboard).push(text);
    }

    fn reveal(&self, _path: &Path) -> bool {
        false
    }

    fn open_external(&self, _path: &Path) -> bool {
        false
    }
}

/// Condvar wake gate shared with `mareader_engine::session::Session`.
#[derive(Clone, Default)]
pub struct Gate {
    inner: Arc<(Mutex<u64>, Condvar)>,
}

impl Gate {
    /// Creates a new condvar wake gate at sequence zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a [`Waker`] that increments the sequence and notifies waiters.
    pub fn waker(&self) -> Waker {
        let inner = Arc::clone(&self.inner);
        Arc::new(move || {
            let (mutex, cvar) = &*inner;
            let mut seq = lock(mutex);
            *seq = seq.wrapping_add(1);
            cvar.notify_all();
        })
    }

    /// Reads the current wake sequence number.
    pub fn seq(&self) -> u64 {
        let (mutex, _) = &*self.inner;
        *lock(mutex)
    }

    /// Waits up to `limit` for the wake sequence to advance beyond `since`.
    pub fn wait_change(&self, since: u64, limit: Duration) {
        let (mutex, cvar) = &*self.inner;
        let guard = lock(mutex);
        let _ = cvar.wait_timeout_while(guard, limit, |seq| *seq == since);
    }
}

/// Running headless engine session paired with its host and wake gate.
pub struct HeadlessRuntime {
    pub session: Box<Session>,
    pub host: Arc<McpHost>,
    pub gate: Gate,
    pub clock: f64,
    persists: bool,
}

impl HeadlessRuntime {
    /// Opens a headless session, resuming any saved document in `state_dir`.
    pub fn open(state_dir: Option<PathBuf>) -> Self {
        let persists = state_dir.is_some();
        let host = Arc::new(McpHost::new(state_dir));
        let gate = Gate::default();
        let shared: Shared = host.clone();
        let mut session = Session::new(shared, gate.waker());
        session.set_progressive_open(false);
        let mut rt = Self {
            session: Box::new(session),
            host,
            gate,
            clock: 0.0,
            persists,
        };
        let _ = rt.settle_jobs();
        if let Some(req) = rt.session.resume_request() {
            rt.session.execute(Command::Open(req), rt.clock);
            let _ = rt.settle_jobs();
        }
        rt
    }

    /// Pumps background worker jobs (open, search, scan) until settled.
    pub fn settle_jobs(&mut self) -> Vec<Event> {
        let deadline = Instant::now() + SETTLE_LIMIT;
        let mut events = Vec::new();
        loop {
            let seq = self.gate.seq();
            self.session.pump();
            let tick = self.session.tick(self.clock);
            self.clock += 0.016;
            let got = self.session.drain();
            let had_events = !got.is_empty();
            events.extend(got);
            if !self.session.busy() && !tick.repaint && !had_events {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            if self.session.busy() {
                self.gate.wait_change(seq, WAIT_STEP);
            }
        }
        events
    }

    /// Persists settings, session resume state, and library changes if enabled.
    pub fn persist(&mut self) {
        if !self.persists {
            return;
        }
        let last = self.session.last_document();
        self.session.settle(last);
        let _ = self.session.drain();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
