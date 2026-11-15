//! The engine's own files: what it writes, where, and how often.
//!
//! A front end hands the session a directory and never writes to it. What
//! outlives a launch is the engine's, so a headless client keeps the same two
//! files and the same repair notes (`docs/architecture.md` § Layers). A
//! platform with no files keeps the same two, by the same names and in the same
//! bytes: [`blobs`] hands them over, and [`parse`] reads them back.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use mareader_settings::Settings;

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
#[derive(Debug, Default)]
pub struct Save {
    dir: Option<PathBuf>,
    wrote: f64,
    dirty: bool,
    failed: Option<String>,
    /// What was read, as the front end last reported it.
    last: Option<LastDocument>,
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

    /// Writes at most every [`EVERY`]; else the wait left, for a frame.
    pub fn write(&mut self, now: f64, settings: &Settings) -> Option<f64> {
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
        self.failed = store(&dir, settings, self.last.as_ref()).err();
        // A refused write is still owed, so the next frame tries again.
        self.dirty = self.failed.is_some();
        None
    }

    /// Writes now: the app is exiting, or a test asks.
    pub fn flush(&mut self, settings: &Settings) {
        self.wrote = f64::NEG_INFINITY;
        self.touch();
        self.write(0.0, settings);
    }

    /// Why a write was refused, once; a caller turns it into a toast.
    pub fn take_failure(&mut self) -> Option<String> {
        self.failed.take()
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
    for (name, text) in blobs(settings, last)? {
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

#[cfg(test)]
#[path = "save_tests.rs"]
mod tests;
