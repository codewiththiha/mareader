//! The reading data a removal kept, put back on the book a later import lands.
//!
//! A removal the reader did not ask to be destructive stows what the library held about the book —
//! the marks, the place they stopped at, the name they gave it — under the file it came from
//! (`crate::storage::kept`). This is the other half of that promise: an import that mints a book of
//! its OWN is that file coming back, and what waits for it lands on the new row.
//!
//! A row an import merges into is not one of those: a book the library still holds has its own
//! marks and its own place, and an arrival that resolves to it is a second copy rather than a book
//! returning. Every minting path therefore hands this the row it just made.

use library_core::book::{Row, book_rows_mut};
use library_core::scan::FoundFile;

use crate::storage::kept::{self as kept_store, KeptBook};

/// The best answer this file gives a waiting record, written onto the row an import has just made
/// and spent by it: a second import of the same file is a second book rather than a second helping
/// of the same reading data.
pub(super) fn reclaim(rows: &mut [Row], file: &FoundFile, row_id: &str) {
    if let Some(kept) = kept_store::claim(file) {
        apply(rows, row_id, kept);
    }
}

/// The write half, split out so a test can hand it a record without a store to have read one from.
fn apply(rows: &mut [Row], row_id: &str, kept: KeptBook) {
    let Some(book) = book_rows_mut(rows).find(|each| each.id == row_id) else {
        return;
    };
    book.page = kept.page;
    book.num_pages = kept.num_pages;
    book.fraction = kept.fraction;
    book.last_read_ms = kept.last_read_ms;
    if let Some(title) = &kept.title {
        // The reader typed this one, so it is locked again: the name the shelf shows must not be
        // overwritten by whatever the document calls itself on the next open.
        book.title = Some(title.clone());
        book.title_locked = true;
    }
    if !kept.marks.is_empty() {
        crate::storage::persist_gloss(row_id, &kept.marks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_core::gloss::{GlossBox, GlossMark, PageAnchor};
    use library_core::book::{Book, find_by_id};
    use library_core::testkit::{book, row};

    fn mark(word: &str) -> GlossMark {
        GlossMark {
            id: word.to_string(),
            word: word.to_string(),
            context: String::new(),
            anchor: PageAnchor {
                page: 3,
                rect: GlossBox {
                    x: 1.0,
                    y: 2.0,
                    w: 3.0,
                    h: 4.0,
                    r: 0.0,
                },
            },
        }
    }

    #[test]
    fn the_place_and_the_name_a_removal_kept_land_on_the_row_the_import_made() {
        let mut rows = vec![row("b2")];
        let gone = Book {
            title: Some("Dune".to_string()),
            title_locked: true,
            page: 42,
            num_pages: 500,
            fraction: Some(0.084),
            last_read_ms: 7,
            ..book("b1")
        };
        apply(&mut rows, "b2", KeptBook::of(&gone, vec![mark("sietch")]));

        let landed = find_by_id(&rows, "b2").expect("the row the import made");
        assert_eq!(landed.page, 42);
        assert_eq!(landed.num_pages, 500);
        assert_eq!(landed.fraction, Some(0.084));
        assert_eq!(landed.last_read_ms, 7);
        assert_eq!(landed.title.as_deref(), Some("Dune"));
        assert!(landed.title_locked, "a name a person typed is not debris");
    }

    #[test]
    fn a_name_the_document_supplied_is_left_to_the_next_open() {
        let mut rows = vec![row("b2")];
        let gone = Book {
            title: Some("A PDF of Dune".to_string()),
            title_locked: false,
            ..book("b1")
        };
        apply(&mut rows, "b2", KeptBook::of(&gone, Vec::new()));
        assert_eq!(
            find_by_id(&rows, "b2").expect("the row the import made").title,
            None,
            "the title the app captured is captured again — this one is not the reader's"
        );
    }
}
