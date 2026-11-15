//! The machine the engine runs on, and the places it has.
//!
//! [`Host`] is all the engine asks of a machine: where its own two files live,
//! where files that can be built again live, the clipboard, the file manager,
//! and the system's own opener. An app answers it — the platform on a desktop,
//! a browser's own table on the web, [`Fake`] in a test. Nothing here names a
//! toolkit and nothing here starts a thread: how a machine does a thing is
//! that machine's business (`docs/architecture.md` § Layers, Law 6).
//!
//! [`Machine`] is what a front end reads back: the places the services it
//! still holds are wired to, each a child of one of the host's two folders.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// What the engine asks of the machine it runs on, and nothing more.
///
/// A dialog is not here: what the reader has to look at is a `UiCommand` a
/// front end draws. Each of these is a thing only a machine can answer.
pub trait Host: Send + Sync {
    /// Where `settings.json` and `session.json` live; none writes neither.
    fn state_dir(&self) -> Option<PathBuf>;

    /// Where files that can be built again live; none asks for nothing.
    fn cache_dir(&self) -> Option<PathBuf>;

    /// Puts `text` on the machine's clipboard.
    fn set_clipboard(&self, text: String);

    /// Shows `path` in the file manager; false when it could not be shown.
    fn reveal(&self, path: &Path) -> bool;

    /// Hands `path` to the system's own player; false when nothing took it.
    fn open_external(&self, path: &Path) -> bool;
}

/// A host with no machine: it keeps what it was asked, and does nothing.
///
/// A test runs a session on one, so nothing it writes lands in the runner's
/// own folders, and what it asked the machine for can be read back.
#[derive(Default)]
pub struct Fake {
    state: Option<PathBuf>,
    cache: Option<PathBuf>,
    copied: Mutex<Vec<String>>,
    shown: Mutex<Vec<PathBuf>>,
    opened: Mutex<Vec<PathBuf>>,
}

impl Fake {
    /// A host with both its folders under `dir`, as a launch has them.
    pub fn under(dir: &Path) -> Self {
        Self {
            state: Some(dir.to_path_buf()),
            cache: Some(dir.join("cache")),
            ..Self::default()
        }
    }

    /// The text put on the clipboard, oldest first.
    pub fn copied(&self) -> Vec<String> {
        kept(&self.copied)
    }

    /// The paths shown in the file manager, oldest first.
    pub fn shown(&self) -> Vec<PathBuf> {
        kept(&self.shown)
    }

    /// The paths handed to the system's opener, oldest first.
    pub fn opened(&self) -> Vec<PathBuf> {
        kept(&self.opened)
    }
}

impl Host for Fake {
    fn state_dir(&self) -> Option<PathBuf> {
        self.state.clone()
    }

    fn cache_dir(&self) -> Option<PathBuf> {
        self.cache.clone()
    }

    fn set_clipboard(&self, text: String) {
        note(&self.copied, text);
    }

    fn reveal(&self, path: &Path) -> bool {
        note(&self.shown, path.to_path_buf());
        true
    }

    fn open_external(&self, path: &Path) -> bool {
        note(&self.opened, path.to_path_buf());
        true
    }
}

fn kept<T: Clone>(held: &Mutex<Vec<T>>) -> Vec<T> {
    held.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn note<T>(held: &Mutex<Vec<T>>, value: T) {
    held.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(value);
}

/// A host shared by a session and whatever else asks the machine.
pub type Shared = Arc<dyn Host>;

/// The places a machine has, and what may run beside them.
///
/// Every path is a directory the host chose; the engine never asks a system
/// where things go, so a browser has an empty machine and a headless client
/// has a temporary one. A place left unnamed stays unattached, and the
/// controller that would have used it keeps its state in memory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Machine {
    /// The library's rows.
    pub library: Option<PathBuf>,
    /// Covers and other files that can be built again.
    pub cache: Option<PathBuf>,
    /// Language packs.
    pub dictionaries: Option<PathBuf>,
    /// The word-level dataset.
    pub vocabulary: Option<PathBuf>,
    /// Downloaded voice models, with the pronunciation pack beside them.
    pub voices: Option<PathBuf>,
    /// The AI service's keys.
    pub ai: Option<PathBuf>,
    /// A helper a debug build may run in the service's place.
    pub ai_helper: Option<PathBuf>,
    /// The voices offered; the catalogue `speech` ships when none is named.
    pub offered: Option<&'static [mareader_speech::Model]>,
}

impl Machine {
    /// A machine with a library in `root` and nothing else: a test's, or a
    /// measurement's.
    pub fn library(root: PathBuf) -> Self {
        Self {
            library: Some(root),
            ..Self::default()
        }
    }

    /// A machine offering `offered` from `dir`, and no other service.
    pub fn voices(dir: PathBuf, offered: &'static [mareader_speech::Model]) -> Self {
        Self {
            voices: Some(dir),
            offered: Some(offered),
            ..Self::default()
        }
    }

    /// The places a launch has, under the two folders its host names.
    ///
    /// `state_dir` holds the engine's own two files and every service's
    /// folder; `cache_dir` is what the platform says may be thrown away.
    pub fn under(host: &dyn Host) -> Self {
        let cache = host.cache_dir();
        let Some(state) = host.state_dir() else {
            return Self {
                cache,
                ..Self::default()
            };
        };
        Self {
            library: Some(state.join("library")),
            cache,
            dictionaries: Some(state.join("dictionaries")),
            vocabulary: Some(state.join("vocabulary")),
            voices: Some(state.join("voice-models")),
            ai: Some(state.join("ai")),
            ai_helper: None,
            offered: None,
        }
    }
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
