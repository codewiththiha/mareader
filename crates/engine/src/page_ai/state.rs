//! What a run keeps: the pages asked, what came, and who asked them.
//!
//! A run's writer lives above, because the service that talks to a model
//! cannot build for a browser, so [`Handle::Live`] holds a [`Writing`] and
//! the front end answers it. The rest is what a page's AI keeps between
//! frames.

use std::collections::HashMap;
use std::sync::Arc;

use mareader_library::{AiPageId, AiTask, BookId, Library, PageSpan};
use mareader_session::Anchor;
use mareader_settings::AiSettings;
use mareader_study::Failure;
use mareader_study::page::{Out, Piece, Rewritten};
use mareader_workspace::PaneId;

use super::Slot;
use super::extract::Source;
use super::view::{Models, Views};

/// Results of documents outside the library, for the session.
pub struct Loose(pub Library);

impl Default for Loose {
    fn default() -> Self {
        Self(Library::new(mareader_library::DeviceId(0)))
    }
}

/// A result being written.
pub struct Run {
    pub id: u64,
    /// The layout its page is counted in: a book's own pages.
    pub layout: u64,
    pub page: usize,
    /// Its last own page: past `page` for a group.
    pub last: usize,
    pub book: Option<BookId>,
    pub name: String,
    pub task: AiTask,
    pub span: PageSpan,
    pub source: Arc<Source>,
    pub by: String,
    pub handle: Option<Handle>,
    /// What has come, as last set, and how many times it came.
    pub shown: Option<Arc<Rewritten>>,
    pub latest: Option<Rewritten>,
    pub shown_at: f64,
    pub generation: u64,
    pub failed: Option<String>,
    /// The kept result it will replace.
    pub redo_of: Option<AiPageId>,
    /// Turned away by a busy model: its group is asked again.
    pub requeue: bool,
}

/// A run's line to its writer.
pub enum Handle {
    /// A writer above: the service's own line to its model.
    Live(Box<dyn Writing>),
    /// Tests and shots: the answer, given on the first look.
    Canned(Option<Rewritten>),
    /// Tests and shots: the answer as words so far, never done.
    Held(Option<Rewritten>),
}

/// A result being written, as the engine sees it: something that yields news.
pub trait Writing {
    /// What has come since the last look.
    fn news(&mut self) -> Vec<News>;
}

/// News from a run.
pub enum News {
    Partial(Rewritten),
    Done(Result<Rewritten, Failure>),
}

pub fn poll(handle: Option<&mut Handle>) -> Vec<News> {
    match handle {
        None => Vec::new(),
        Some(Handle::Canned(answer)) => answer
            .take()
            .map(|r| News::Done(Ok(r)))
            .into_iter()
            .collect(),
        Some(Handle::Held(answer)) => answer.take().map(News::Partial).into_iter().collect(),
        Some(Handle::Live(live)) => live.news(),
    }
}

/// The page's own text in capitals: a stand-in answer.
pub fn canned(source: &Source) -> Rewritten {
    let outs = source
        .pieces
        .iter()
        .enumerate()
        .map(|(i, piece)| match piece {
            Piece::Text { role, text, .. } => Out::Text {
                role: *role,
                text: text.to_uppercase(),
                code: Vec::new(),
                from: Some(i),
            },
            Piece::Kept(_) => Out::Kept(i),
        });
    Rewritten {
        outs: outs.collect(),
        missing: 0,
    }
}

/// The result a pane shows over a page, and since when.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Showing {
    pub slot: Slot,
    pub since: f64,
}

/// A book's results by the page each starts on, for one layout.
pub struct Index {
    pub key: u64,
    /// The results' stamp and book it was counted at: unchanged, no recount.
    pub stamp: (u64, BookId),
    pub pages: Arc<HashMap<usize, Vec<AiPageId>>>,
}

/// A pane's layout, and each page's bubbles with their own pages.
pub type Drawn = (u64, Vec<(usize, Vec<(Slot, usize)>)>);

/// A pane, a layout and one of its own pages.
pub type Key = (PaneId, u64, usize);

/// What a result covers: a book's text, or a PDF's pages, both in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cover {
    Text(Anchor, Anchor),
    Pages(usize, usize),
}

impl Cover {
    pub fn meets(self, other: Self) -> bool {
        match (self, other) {
            (Self::Text(a, b), Self::Text(c, d)) => a < d && c < b,
            (Self::Pages(a, b), Self::Pages(c, d)) => a <= d && c <= b,
            _ => false,
        }
    }
}

/// A page to work on, as the app gathers it.
pub struct Asked<'a> {
    pub pane: PaneId,
    pub page: usize,
    pub book: Option<BookId>,
    /// The document's name: a session's results are kept by it.
    pub name: &'a str,
    pub documents: &'a dyn Views,
    pub ai: &'a dyn Models,
    pub settings: &'a AiSettings,
}

/// Who a run is for, and what it asks with.
pub struct Who<'a> {
    pub pane: PaneId,
    pub layout: u64,
    pub book: Option<BookId>,
    pub name: &'a str,
    pub ai: &'a dyn Models,
    pub settings: &'a AiSettings,
}

/// What a continuous run looks at each frame.
pub struct Around<'a> {
    pub main: &'a Library,
    pub documents: &'a dyn Views,
    pub ai: &'a dyn Models,
    pub settings: &'a AiSettings,
}
