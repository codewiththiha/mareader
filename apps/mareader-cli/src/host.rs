//! Headless `Host` implementation and condvar waker over `std`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use mareader_engine::host::Host;
use mareader_engine::session::Waker;

/// Machine adapter over `std` with no display, GPU, or toolkit dependencies.
pub struct CliHost {
    state: Option<PathBuf>,
    cache: Option<PathBuf>,
    clipboard: Mutex<Vec<String>>,
}

impl CliHost {
    pub fn new(state: Option<PathBuf>) -> Self {
        let cache = state.as_ref().map(|dir| dir.join("cache"));
        Self {
            state,
            cache,
            clipboard: Mutex::new(Vec::new()),
        }
    }

    pub fn take_clipboard(&self) -> Vec<String> {
        let mut guard = lock(&self.clipboard);
        std::mem::take(&mut *guard)
    }
}

impl Host for CliHost {
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

/// Condvar wake gate shared between engine worker threads and the CLI loop.
#[derive(Clone, Default)]
pub struct Gate {
    inner: Arc<(Mutex<u64>, Condvar)>,
}

impl Gate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn waker(&self) -> Waker {
        let inner = Arc::clone(&self.inner);
        Arc::new(move || {
            let (mutex, cvar) = &*inner;
            let mut seq = lock(mutex);
            *seq = seq.wrapping_add(1);
            cvar.notify_all();
        })
    }

    pub fn seq(&self) -> u64 {
        let (mutex, _) = &*self.inner;
        *lock(mutex)
    }

    pub fn wait_change(&self, since: u64, limit: Duration) {
        let (mutex, cvar) = &*self.inner;
        let guard = lock(mutex);
        let _ = cvar.wait_timeout_while(guard, limit, |seq| *seq == since);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
