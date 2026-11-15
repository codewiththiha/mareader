//! Finding words in the notes without drawing them.

pub mod query;

use mareader_library::Spot;
use mareader_pdf::active_entry;
use mareader_session::{Anchor, Document};

/// The page `spot` lands on in `doc`, if any.
pub fn page_of(doc: &Document, spot: Spot) -> Option<usize> {
    match spot {
        Spot::Page { page, .. } => Some(page),
        Spot::Chars {
            from: [page, _], ..
        } => Some(page),
        Spot::Text { block, start, .. } => doc.locate(Anchor {
            block,
            offset: start,
        }),
        Spot::Blocks {
            from: [block, offset],
            ..
        } => doc.locate(Anchor { block, offset }),
    }
}

/// Where `page` starts: its first text when the text reflows.
pub(crate) fn bookmark_spot(doc: &Document, page: usize) -> Spot {
    match doc.anchor(page) {
        Some(a) => Spot::Text {
            block: a.block,
            start: a.offset,
            end: a.offset,
        },
        None => Spot::Page {
            page,
            rect: [0.0; 4],
        },
    }
}

/// The chapter `page` falls in, from the outline.
pub fn bookmark_chapter(doc: &Document, page: usize) -> Option<String> {
    let outline = doc.outline();
    let entry = active_entry(outline, page).and_then(|i| outline.get(i));
    let title = entry.map(|e| e.title.trim().to_owned());
    title.filter(|t| !t.is_empty())
}

/// A bookmark's name: its chapter, else its page.
pub(crate) fn bookmark_label(doc: &Document, page: usize) -> String {
    bookmark_chapter(doc, page).unwrap_or_else(|| format!("Page {}", page + 1))
}
