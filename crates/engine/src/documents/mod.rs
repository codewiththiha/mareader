//! Open documents, per-pane reading places, typography, and reload tracking.

mod poll;
mod queries;
mod retype;
pub mod state;

use std::path::PathBuf;
use std::sync::Arc;

use mareader_library::BookId;
use mareader_session::{Anchor, Source, SpinePlace};
use mareader_workspace::{Edge, PaneId};

pub use poll::UPLOADS;
pub use queries::PaneShow;
pub use state::{Opening, Sink, Slot, State};

/// Where an open lands, measured from its pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    #[default]
    Into,
    /// A new pane to the right, else below when the right has no room.
    Beside,
    /// A new pane on exactly this edge, as a drop asks.
    Edge(Edge),
}

/// A request to show a file in a pane (the active one by default).
#[derive(Debug, Clone, PartialEq)]
pub struct OpenRequest {
    pub source: Source,
    pub name: Option<String>,
    pub pane: Option<PaneId>,
    /// Into that pane, or a new one split from it.
    pub placement: Placement,
    /// The page to show once open; past the end, the last.
    pub page: usize,
    /// A book's place to show instead, found again in today's type.
    pub place: Option<Anchor>,
    /// A stable EPUB location that avoids parsing the whole prefix.
    pub spine: Option<SpinePlace>,
    /// The last known count, for a preview's global page display.
    pub pages: usize,
    /// Read as this library book, a twin perhaps; none: the file's own.
    pub book: Option<BookId>,
}

/// A document's current global position and resume data.
pub struct DocumentPlace {
    /// The source file.
    pub path: PathBuf,
    /// The global page currently shown.
    pub page: usize,
    /// A legacy block anchor, when the document is complete.
    pub anchor: Option<Anchor>,
    /// A stable EPUB location, when available.
    pub spine: Option<SpinePlace>,
    /// The known or estimated full page count.
    pub pages: usize,
}

impl OpenRequest {
    pub fn path(path: PathBuf) -> Self {
        Self {
            source: Source::Path(path),
            name: None,
            pane: None,
            placement: Placement::Into,
            page: 0,
            place: None,
            spine: None,
            pages: 0,
            book: None,
        }
    }

    pub fn bytes(bytes: Vec<u8>, name: impl Into<String>) -> Self {
        Self {
            source: Source::Bytes(Arc::new(bytes)),
            name: Some(name.into()),
            pane: None,
            placement: Placement::Into,
            page: 0,
            place: None,
            spine: None,
            pages: 0,
            book: None,
        }
    }

    /// The file it reads from, if it reads from one.
    pub fn file(&self) -> Option<PathBuf> {
        match &self.source {
            Source::Path(p) => Some(p.clone()),
            Source::Bytes(_) => None,
        }
    }

    /// What to call it while it opens.
    pub fn label(&self) -> String {
        let path = match &self.source {
            Source::Path(p) => Some(p.to_string_lossy().into_owned()),
            Source::Bytes(_) => None,
        };
        let name = self.name.clone().or(path);
        name.as_deref()
            .and_then(mareader_pdf::file_stem)
            .unwrap_or_else(|| "document".into())
    }
}
