//! Where a pane is in its document: the reading half of a view.
//!
//! This is what a reopened window and a headless client both need — which
//! page, how far through, the book it reads as, how it is turned. The
//! geometry a front end needs to draw it (scroll offsets, laid-out rects,
//! textures) and the ids of what is selected stay above the line.

use mareader_library::BookId;
use mareader_reflow::Anchor;
use mareader_session::{DocId, Selection};

use crate::pane::ViewMode;

/// Where one pane is in one document.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub doc: DocId,
    /// The library book this pane reads as; none reads the file itself.
    pub book: Option<BookId>,
    pub mode: ViewMode,
    /// The page read: a paged mode's page, or the one under the centre.
    pub current: usize,
    /// Pages on screen, when the front end last drew it.
    pub visible: std::ops::Range<usize>,
    /// How far through the document, 0 to 1.
    pub progress: f32,
    /// The page a find left off on.
    pub landed: Option<usize>,
    /// The document's pages turn right to left.
    pub rtl: bool,
    /// In a spread, the pages before the pairing starts.
    pub lead: usize,
    /// Last drawn sharp zoom factor for page-AI rendering.
    pub zoom: f32,
    /// Selected text in this pane, if any.
    pub selection: Option<Selection>,
}

impl Place {
    pub fn new(doc: DocId) -> Self {
        Self {
            doc,
            book: None,
            mode: ViewMode::default(),
            current: 0,
            visible: 0..0,
            progress: 0.0,
            landed: None,
            rtl: false,
            lead: 0,
            zoom: 1.0,
            selection: None,
        }
    }

    /// The page a turn from `current` lands on, given the document's `count`.
    pub fn turned(&self, forward: bool, count: usize) -> usize {
        self.mode.turn(self.current, forward, (count, self.lead))
    }

    /// Jumps to `page`, kept within the document's `count`.
    pub fn go_to(&mut self, page: usize, count: usize) {
        self.current = page.min(count.saturating_sub(1));
    }

    /// Reading the whole page: how far through the document, 0 to 1.
    pub fn advanced(&self, count: usize) -> f32 {
        match count {
            0 => 0.0,
            n => (self.current + 1) as f32 / n as f32,
        }
    }

    /// A place to come back to: the page, and how far down it.
    pub fn anchor(&self) -> Anchor {
        Anchor {
            block: self.current,
            offset: 0,
        }
    }
}
