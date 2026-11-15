//! The frame's report from a pane's pages to the study tools.

use egui::{Pos2, Rect};
use mareader_appearance::Rgb;
use mareader_library::{MarkId, Pen};

/// What a pane's pages report to the study tools this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Gloss {
    /// Explain was pressed on the pane's selection.
    Explain,
    /// Look up was pressed on the pane's selection.
    LookUp,
    /// Note was pressed on the pane's selection.
    Note,
    /// Read aloud was pressed: the selection alone is read.
    ReadAloud,
    /// The read-aloud pin rested, or was clicked: read from it.
    ReadFrom(mareader_engine::read_aloud::ReadFrom),
    /// The selection reached the clipboard.
    Copied,
    /// A mark was clicked: its card, beside it.
    Open {
        mark: MarkId,
        at: Rect,
    },
    /// A mark was right-clicked at `at`; `words` are its bounds.
    Menu {
        mark: MarkId,
        at: Pos2,
        words: Rect,
    },
    /// A highlighter in the selection bar; a colour if one was picked.
    Highlight(Pen, Option<Rgb>),
    /// Where the open card's mark is now; none while its text loads.
    Anchor(Option<Rect>),
    /// A long press on a mark: picking several begins with it.
    Pick(MarkId),
    /// A click on a mark while picking.
    Toggle(MarkId),
    /// While picking: the marks on screen, and every mark of the book.
    Seen {
        page: Vec<MarkId>,
        all: Vec<MarkId>,
    },
    /// A shaded word was clicked: its characters on `page`.
    Shaded {
        page: usize,
        chars: std::ops::Range<usize>,
        at: Rect,
    },
    /// The pointer rested on a shaded word, in its sentence.
    Rest {
        word: String,
        context: String,
        at: Rect,
    },
    /// A hold on empty space lifted the pane, to be moved.
    Lift(Pos2),
    /// A right-click on a page, off its marks and selection.
    PageMenu {
        page: usize,
        at: Pos2,
        /// The book's text under the pointer, if any.
        place: Option<mareader_session::Anchor>,
        /// The character under the pointer, on a line of text.
        caret: Option<usize>,
    },
    /// A page's AI bubble was clicked, or right-clicked.
    Bubble {
        page: usize,
        index: usize,
        at: Pos2,
    },
    BubbleMenu {
        page: usize,
        index: usize,
        at: Pos2,
    },
    /// The continuous run's pill was pressed.
    Pill(crate::page_ai::PillPress),
}
