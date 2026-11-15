//! Where the words being read are, for a pane to light them.

use mareader_session::Anchor;
use mareader_speech::Spot;

/// The words being read, to paint in their pane.
#[derive(Clone, Debug, PartialEq)]
pub struct Lit {
    pub span: Span,
}

/// Where the words read are: by place in the book, or on its pages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Span {
    Anchors { start: Anchor, end: Anchor },
    Chars { start: Spot, end: Spot },
}
