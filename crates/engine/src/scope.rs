//! Which settings apply, from what the focused pane and workspace show.

/// What the focused pane shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shown {
    #[default]
    Nothing,
    Pdf,
    /// A book; `stream` while read as one vertical column.
    Book {
        stream: bool,
    },
}

/// The workspace as the settings window sees it this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    pub focused: Shown,
    pub split: bool,
    /// Some pane takes text settings: a book, or a PDF's AI page shown.
    pub any_text: bool,
    /// The focused PDF shows an AI page, which is set in its type.
    pub ai_pages: bool,
    /// The focused pane scrolls sideways, where pages have no margin.
    pub sideways: bool,
    /// The AI Dictionary has a model to ask, so it marks words.
    pub explains: bool,
    /// The focused book in the library, and its own design choice.
    pub book_design: Option<(mareader_library::BookId, Option<mareader_library::Design>)>,
}

/// When a setting applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applies {
    Always,
    /// PDF machinery: hidden while a book is focused.
    NotBook,
    /// Text settings: only while a book is focused.
    Book,
    /// The type of a focused PDF's AI pages.
    AiPages,
    /// Separate pages, not a book's vertical stream.
    Pages,
    Split,
    /// Split, with text settings reaching some pane.
    SplitWithText,
}

impl Scope {
    pub fn book(&self) -> bool {
        matches!(self.focused, Shown::Book { .. })
    }

    pub fn applies(&self, applies: Applies) -> bool {
        match applies {
            Applies::Always => true,
            Applies::NotBook => !self.book(),
            Applies::Book => self.book(),
            Applies::AiPages => !self.book() && self.ai_pages,
            Applies::Pages => self.focused != Shown::Book { stream: true },
            Applies::Split => self.split,
            Applies::SplitWithText => self.split && self.any_text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_focused_document_decides_what_applies() {
        let pdf = Scope {
            focused: Shown::Pdf,
            ..Scope::default()
        };
        let stream = Scope {
            focused: Shown::Book { stream: true },
            ..pdf
        };
        let paged = Scope {
            focused: Shown::Book { stream: false },
            ..pdf
        };
        assert!(pdf.applies(Applies::NotBook) && !pdf.applies(Applies::Book));
        assert!(stream.applies(Applies::Book) && !stream.applies(Applies::NotBook));
        // A book's single, spread and sideways views have pages.
        assert!(pdf.applies(Applies::Pages) && paged.applies(Applies::Pages));
        assert!(!stream.applies(Applies::Pages));
        // Nothing open reads as a PDF would.
        assert!(Scope::default().applies(Applies::NotBook));
        // A PDF's AI pages take type; a book's own settings cover them.
        let ai = Scope {
            ai_pages: true,
            ..pdf
        };
        assert!(ai.applies(Applies::AiPages) && !pdf.applies(Applies::AiPages));
        let book = Scope {
            ai_pages: true,
            ..paged
        };
        assert!(!book.applies(Applies::AiPages));
    }

    #[test]
    fn pane_text_needs_a_split_with_text_somewhere() {
        let split = Scope {
            focused: Shown::Pdf,
            split: true,
            ..Scope::default()
        };
        assert!(!split.applies(Applies::SplitWithText), "only PDFs");
        let book = Scope {
            any_text: true,
            ..split
        };
        assert!(
            book.applies(Applies::SplitWithText),
            "the focused one is a PDF"
        );
        let alone = Scope {
            split: false,
            ..book
        };
        assert!(!alone.applies(Applies::SplitWithText));
    }
}
