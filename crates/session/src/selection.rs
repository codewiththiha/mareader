//! A selection's two ends and the text between them; no window needed.

use std::ops::Range;
use std::sync::Arc;

use crate::TextLayer;

/// A spot between two characters of a page's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Caret {
    pub page: usize,
    pub index: usize,
}

/// Where a drag began and where it is now; either may come first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Caret,
    pub focus: Caret,
}

impl Selection {
    /// Characters `range` of one page.
    pub fn within(page: usize, range: Range<usize>) -> Self {
        Self {
            anchor: Caret {
                page,
                index: range.start,
            },
            focus: Caret {
                page,
                index: range.end,
            },
        }
    }

    pub fn ordered(self) -> (Caret, Caret) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    pub fn is_empty(self) -> bool {
        self.anchor == self.focus
    }

    /// The first and last page touched, for anything acting on them.
    pub fn pages(self) -> Range<usize> {
        let (a, b) = self.ordered();
        a.page..b.page + 1
    }

    /// The characters selected on `page`, of its `len`; none if empty.
    pub fn on_page(self, page: usize, len: usize) -> Option<Range<usize>> {
        let (a, b) = self.ordered();
        if !(a.page..=b.page).contains(&page) {
            return None;
        }
        let start = if page == a.page { a.index.min(len) } else { 0 };
        let end = if page == b.page {
            b.index.min(len)
        } else {
            len
        };
        (start < end).then_some(start..end)
    }

    /// The selected text, pages joined by line breaks unless one goes on.
    pub fn text(self, mut layer: impl FnMut(usize) -> Option<Arc<TextLayer>>) -> String {
        let mut out = String::new();
        let mut first = true;
        for page in self.pages() {
            let Some(l) = layer(page) else { continue };
            if let Some(range) = self.on_page(page, l.text.len()) {
                if !first && !l.text.continues() {
                    out.push('\n');
                }
                out.push_str(&l.text.text(range));
                first = false;
            }
        }
        out
    }
}
