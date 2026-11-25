//! The engine's own files: what it writes, where, and how often.
//!
//! A front end hands the session a directory and never writes to it. What
//! outlives a launch is the engine's, so a headless client keeps the same two
//! files and the same repair notes (`docs/architecture.md` section Layers). A
//! platform with no files keeps the same two, by the same names and in the same
//! bytes: [`blobs`] hands them over, and [`parse`] reads them back.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use mareader_settings::Settings;

use crate::session::Waker;

/// Settings reach disk this long after a change, at most.
pub const EVERY: f64 = 0.5;

/// The user's choices, in `mareader_settings`' schema.
pub const SETTINGS_FILE: &str = "settings.json";
/// What was read at exit, so the next launch reopens it.
pub const SESSION_FILE: &str = "session.json";

/// The two files' text, each beside the name it is kept under.
pub type Blobs = [(&'static str, String); 2];

/// What reopens at launch: the file read at exit, on its page.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LastDocument {
    pub path: PathBuf,
    pub page: usize,
    /// A book's block and offset, which outlast a change of type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<(usize, usize)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spine: Option<mareader_library::SpinePosition>,
    #[serde(default)]
    pub pages: usize,
    /// The library book it was read as, so a twin reopens as itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub book: Option<mareader_library::BookId>,
}

/// What a launch found: the choices, what to reopen, and any repair.
#[derive(Debug, Default)]
pub struct Loaded {
    pub settings: Settings,
    pub last: Option<LastDocument>,
    /// What could not be read, for the log and a toast.
    pub notes: Vec<String>,
}

/// The engine's writing: where, when it last did, and whether it owes one.
pub struct Save {
    dir: Option<PathBuf>,
    wrote: f64,
    dirty: bool,
    failed: Option<String>,
    /// What was read, as the front end last reported it.
    last: Option<LastDocument>,
    inline: bool,
    #[cfg(not(target_arch = "wasm32"))]
    waker: Option<Waker>,
    #[cfg(not(target_arch = "wasm32"))]
    writer: Option<writer::Writer>,
}

impl std::fmt::Debug for Save {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Save")
            .field("dir", &self.dir)
            .field("wrote", &self.wrote)
            .field("dirty", &self.dirty)
            .field("failed", &self.failed)
            .field("last", &self.last)
            .field("inline", &self.inline)
            .finish()
    }
}

impl Default for Save {
    fn default() -> Self {
        Self {
            dir: None,
            wrote: 0.0,
            dirty: false,
            failed: None,
            last: None,
            inline: true,
            #[cfg(not(target_arch = "wasm32"))]
            waker: None,
            #[cfg(not(target_arch = "wasm32"))]
            writer: None,
        }
    }
}

impl Save {
    /// Writes into `dir`; none where a platform has no files, as a browser.
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            wrote: f64::NEG_INFINITY,
            last: None,
            ..Self::default()
        }
    }

    /// Runs writes inline on this thread when `on`, else on a worker thread.
    pub fn set_inline(&mut self, on: bool) {
        if on {
            self.drain(false);
        }
        self.inline = on;
    }

    /// Registers a wake callback notified when a background write fails.
    pub fn set_waker(&mut self, waker: Waker) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.waker = Some(waker);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = waker;
    }

    /// Where the two files live, if they live anywhere.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// What was read, as the front end last reported it.
    pub fn last(&self) -> Option<&LastDocument> {
        self.last.as_ref()
    }

    /// Something persisted changed; the next due write carries it.
    pub fn touch(&mut self) {
        self.dirty = true;
    }

    /// Whether a write is owed.
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Remembers what was read; a difference owes a write.
    pub fn note(&mut self, last: Option<LastDocument>) {
        if self.last == last {
            return;
        }
        self.last = last;
        self.touch();
    }

    /// Collects any background write error so callers can retry and report it.
    pub fn poll(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(why) = self.writer.as_mut().and_then(writer::Writer::take_failure) {
            self.failed = Some(why);
            // A refused write is still owed, so the next frame tries again.
            self.dirty = true;
        }
    }

    /// Writes at most every [`EVERY`]; else the wait left, for a frame.
    pub fn write(&mut self, now: f64, settings: &Settings) -> Option<f64> {
        self.poll();
        if self.dir.is_none() || !self.dirty {
            return None;
        }
        let wait = self.wrote + EVERY - now;
        if wait > 0.0 {
            return Some(wait);
        }
        self.wrote = now;
        self.dirty = false;
        let dir = self.dir.clone()?;
        let encoded = match blobs(settings, self.last.as_ref()) {
            Ok(encoded) => encoded,
            Err(why) => {
                self.failed = Some(why);
                self.dirty = true;
                return None;
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        if !self.inline {
            let writer = self.writer.get_or_insert_with(writer::Writer::spawn);
            writer.enqueue(dir, encoded, self.waker.as_ref());
            return None;
        }
        self.failed = write_blobs(&dir, &encoded).err();
        // A refused write is still owed, so the next frame tries again.
        self.dirty = self.failed.is_some();
        None
    }

    /// Writes now: the app is exiting, or a test asks.
    pub fn flush(&mut self, settings: &Settings) {
        self.drain(true);
        self.wrote = f64::NEG_INFINITY;
        self.touch();
        let was_inline = self.inline;
        self.inline = true;
        self.write(0.0, settings);
        self.inline = was_inline;
    }

    /// Why a write was refused, once; a caller turns it into a toast.
    pub(crate) fn take_failure(&mut self) -> Option<String> {
        self.poll();
        self.failed.take()
    }

    fn drain(&mut self, drop_pending: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(writer) = &mut self.writer {
            writer.drain(drop_pending);
            self.poll();
        }
        #[cfg(target_arch = "wasm32")]
        let _ = drop_pending;
    }
}

/// Reads the two files; anything unreadable falls back, with a note.
pub fn load(dir: Option<&Path>) -> Loaded {
    let Some(dir) = dir else {
        return Loaded::default();
    };
    let mut notes = Vec::new();
    let choices = read(dir, SETTINGS_FILE, &mut notes);
    let session = read(dir, SESSION_FILE, &mut notes);
    let mut loaded = parse(choices, session);
    loaded.notes.extend(notes);
    loaded
}

/// The two files' text made into a launch, wherever they were kept. A session
/// file holding `null` is a flush that had nothing open: it reopens nothing and
/// says nothing, because the engine wrote those four bytes itself.
pub fn parse(settings: Option<String>, session: Option<String>) -> Loaded {
    let mut notes = Vec::new();
    let choices = match settings {
        None => Settings::default(),
        Some(json) => {
            let loaded = mareader_settings::load(&json);
            notes.extend(loaded.notes);
            loaded.settings
        }
    };
    let last = match session {
        None => None,
        Some(json) => match serde_json::from_str::<Option<LastDocument>>(&json) {
            Ok(last) => last,
            Err(why) => {
                notes.push(format!("{SESSION_FILE} could not be read: {why}"));
                None
            }
        },
    };
    Loaded {
        settings: choices,
        last,
        notes,
    }
}

/// The two files' text, as a flush writes them and a platform with no files
/// keeps them: the names and the bytes are the engine's either way.
pub fn blobs(settings: &Settings, last: Option<&LastDocument>) -> Result<Blobs, String> {
    let choices = mareader_settings::to_json(settings);
    let session = serde_json::to_string(&last).map_err(|e| format!("{SESSION_FILE}: {e}"))?;
    Ok([(SETTINGS_FILE, choices), (SESSION_FILE, session)])
}

/// Both files whole: a temp beside each, then a rename.
pub fn store(dir: &Path, settings: &Settings, last: Option<&LastDocument>) -> Result<(), String> {
    let encoded = blobs(settings, last)?;
    write_blobs(dir, &encoded)
}

fn write_blobs(dir: &Path, encoded: &Blobs) -> Result<(), String> {
    for (name, text) in encoded {
        write(dir, name, text.as_bytes())?;
    }
    Ok(())
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    mareader_library::disk::write_atomic(&dir.join(name), bytes).map_err(|e| format!("{name}: {e}"))
}

/// A file's text; none when there is no file yet, as at a first launch.
fn read(dir: &Path, name: &str, notes: &mut Vec<String>) -> Option<String> {
    match std::fs::read_to_string(dir.join(name)) {
        Ok(text) => Some(text),
        Err(why) if why.kind() == ErrorKind::NotFound => None,
        Err(why) => {
            notes.push(format!("{name} could not be read: {why}"));
            None
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod writer {
    use std::fmt;
    use std::path::PathBuf;
    use std::sync::{Arc, Condvar, Mutex, PoisonError};
    use std::thread::JoinHandle;

    use crate::session::Waker;

    use super::{Blobs, write_blobs};

    struct Slot {
        pending: Option<(PathBuf, Blobs)>,
        writing: bool,
        closed: bool,
        failed: Option<String>,
        waker: Option<Waker>,
    }

    /// Coalescing off-thread writer so atomic fsyncs never stall the UI frame.
    pub(super) struct Writer {
        shared: Arc<(Mutex<Slot>, Condvar)>,
        handle: Option<JoinHandle<()>>,
    }

    impl fmt::Debug for Writer {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("Writer")
        }
    }

    fn lock(mutex: &Mutex<Slot>) -> std::sync::MutexGuard<'_, Slot> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    impl Writer {
        pub(super) fn spawn() -> Self {
            let shared = Arc::new((
                Mutex::new(Slot {
                    pending: None,
                    writing: false,
                    closed: false,
                    failed: None,
                    waker: None,
                }),
                Condvar::new(),
            ));
            let worker = Arc::clone(&shared);
            let handle = std::thread::Builder::new()
                .name("mareader-save".into())
                .spawn(move || run(&worker))
                .ok();
            Self { shared, handle }
        }

        pub(super) fn enqueue(&mut self, dir: PathBuf, blobs: Blobs, waker: Option<&Waker>) {
            if self.handle.is_none() {
                let err = write_blobs(&dir, &blobs).err();
                lock(&self.shared.0).failed = err;
                return;
            }
            let (mutex, cvar) = &*self.shared;
            let mut slot = lock(mutex);
            slot.pending = Some((dir, blobs));
            if let Some(waker) = waker {
                slot.waker = Some(Arc::clone(waker));
            }
            cvar.notify_one();
        }

        pub(super) fn take_failure(&mut self) -> Option<String> {
            lock(&self.shared.0).failed.take()
        }

        pub(super) fn drain(&mut self, drop_pending: bool) {
            let (mutex, cvar) = &*self.shared;
            let mut slot = lock(mutex);
            if drop_pending {
                slot.pending = None;
            }
            while slot.writing || (!drop_pending && slot.pending.is_some()) {
                slot = cvar.wait(slot).unwrap_or_else(PoisonError::into_inner);
            }
        }
    }

    impl Drop for Writer {
        fn drop(&mut self) {
            let (mutex, cvar) = &*self.shared;
            {
                let mut slot = lock(mutex);
                slot.closed = true;
                cvar.notify_all();
            }
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn run(shared: &(Mutex<Slot>, Condvar)) {
        let (mutex, cvar) = shared;
        loop {
            let job = {
                let mut slot = lock(mutex);
                while slot.pending.is_none() && !slot.closed {
                    slot = cvar.wait(slot).unwrap_or_else(PoisonError::into_inner);
                }
                match slot.pending.take() {
                    Some(job) => {
                        slot.writing = true;
                        job
                    }
                    None => return,
                }
            };
            let err = write_blobs(&job.0, &job.1).err();
            let wake = {
                let mut slot = lock(mutex);
                let wake = err.as_ref().and_then(|_| slot.waker.clone());
                if err.is_some() {
                    slot.failed = err;
                }
                wake
            };
            if let Some(wake) = wake {
                wake();
            }
            let mut slot = lock(mutex);
            slot.writing = false;
            cvar.notify_all();
        }
    }
}

#[cfg(test)]
#[path = "save_tests.rs"]
mod tests;
