//! Active read-aloud playback session: narrated recordings or speech voice.

use std::ops::Range;
use std::path::{Path, PathBuf};

use mareader_library::BookId;
use mareader_session::Anchor;
use mareader_workspace::PaneId;

use super::lit::Lit;
use super::narrated::Narrated;
use super::place::{Stand, place_name};
use super::player::{Act, Lyrics, View};
use super::spoken::Spoken;

/// The book being read, as the library and files know it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Origin {
    pub book: Option<BookId>,
    pub path: Option<PathBuf>,
    pub name: String,
}

impl Origin {
    /// Whether a pane showing `book` from `path` shows this book.
    pub fn is(&self, book: Option<BookId>, path: Option<&Path>) -> bool {
        let same_book = self.book.is_some() && book == self.book;
        let same_file = self.path.is_some() && path == self.path.as_deref();
        same_book || same_file
    }
}

/// What the player card says about the book being read.
pub struct About<'a> {
    pub title: &'a str,
    pub place: Option<String>,
    pub narrator: Option<&'a str>,
    pub playing: bool,
}

/// What reads: the book's narration, or a voice saying its text.
pub(super) enum Active {
    Narrated(Narrated),
    Spoken(Box<Spoken>),
}

impl Active {
    pub(super) fn pane(&self) -> PaneId {
        match self {
            Self::Narrated(s) => s.pane,
            Self::Spoken(s) => s.pane,
        }
    }

    pub(super) fn playing(&self) -> bool {
        match self {
            Self::Narrated(s) => !s.paused && !s.finished,
            Self::Spoken(s) => s.playing(),
        }
    }

    pub(super) fn toggle(&mut self) {
        match self {
            Self::Narrated(s) => s.toggle(),
            Self::Spoken(s) => s.toggle(),
        }
    }

    pub(super) fn pause(&mut self) {
        match self {
            Self::Narrated(s) => s.pause(),
            Self::Spoken(s) => s.pause(),
        }
    }

    pub(super) fn hint(&self, share: f32) -> Option<String> {
        match self {
            Self::Narrated(s) => s.hint(share),
            Self::Spoken(s) => s.hint(share),
        }
    }

    pub(super) fn ours(&self, stand: &Stand) -> bool {
        match self {
            Self::Narrated(s) => s.ours(stand),
            Self::Spoken(s) => s.ours(stand),
        }
    }

    pub(super) fn rehome(&mut self, stand: &Stand) -> bool {
        match self {
            Self::Narrated(s) => s.rehome(stand),
            Self::Spoken(s) => s.rehome(stand),
        }
    }

    pub(super) fn view<'a>(&self, hint: &'a dyn Fn(f32) -> Option<String>) -> View<'a> {
        match self {
            Self::Narrated(s) => s.view(hint),
            Self::Spoken(s) => s.view(hint),
        }
    }

    pub(super) fn ask_cost(&self, rate: f32) -> Option<String> {
        match self {
            Self::Spoken(s) => s.ask_cost(rate),
            Self::Narrated(_) => None,
        }
    }

    pub(super) fn act(&mut self, act: Act) -> bool {
        match self {
            Self::Narrated(s) => s.act(act),
            Self::Spoken(s) => s.act(act),
        }
    }

    pub(super) fn spot(&self) -> Option<(usize, Option<Range<usize>>)> {
        match self {
            Self::Narrated(s) => Some((s.page()?, None)),
            Self::Spoken(s) => s.spot().map(|(page, chars)| (page, Some(chars))),
        }
    }

    pub(super) fn said(&self) -> Option<Anchor> {
        match self {
            Self::Narrated(s) => s.said(),
            Self::Spoken(s) => s.said(),
        }
    }

    pub(super) fn lit(&self) -> Option<Lit> {
        match self {
            Self::Narrated(s) => s.lit(),
            Self::Spoken(s) => s.lit(),
        }
    }

    pub(super) fn about<'a>(&'a self, title: &'a str) -> About<'a> {
        let (place, narrator, playing) = match self {
            Self::Narrated(s) => {
                let place = s.page().map(|p| place_name(&s.document, p));
                let playing = !s.paused && !s.finished;
                (place, s.narration.narrator.as_deref(), playing)
            }
            Self::Spoken(s) => (s.whereabouts(), None, s.playing()),
        };
        About {
            title,
            place,
            narrator,
            playing,
        }
    }

    pub(super) fn lyrics(&self) -> Option<Lyrics> {
        match self {
            Self::Narrated(_) => None,
            Self::Spoken(s) => s.lyrics(),
        }
    }
}
