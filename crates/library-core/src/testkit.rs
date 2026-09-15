//! Fixture builders for the library's host tests — one spelling of "a book",
//! "a row", "a link" and "a shelf" for every test module that used to build its
//! own.

use crate::book::{Book, Fingerprint, Origin, Row};
use crate::scan::FoundFile;
use crate::shelf::{Shelf, ShelfKind};
use reader_core::format::Format;

/// The neutral fingerprint: every field `1`.
pub fn fingerprint() -> Fingerprint {
    Fingerprint {
        size: 1,
        mtime_ms: 1,
        head_hash: 1,
    }
}

/// A fingerprint whose three fields all read `n`.
pub fn fp_n(n: u32) -> Fingerprint {
    Fingerprint {
        size: u64::from(n),
        mtime_ms: u64::from(n),
        head_hash: n,
    }
}

/// A minimal linked book: id `id`, address `/books/{id}.pdf`, format PDF, joined
/// at `0`, page 1, nothing read, nothing missing, nothing pending.
pub fn book(id: &str) -> Book {
    Book::new(
        id.to_string(),
        fingerprint(),
        Format::Pdf,
        Origin::Linked {
            src: format!("/books/{id}.pdf"),
        },
        0,
    )
}

pub fn row(id: &str) -> Row {
    Row::Book(book(id))
}

/// A linked Markdown book at `/books/{id}.md`, the fixture an app-side test lands through a placement.
pub fn markdown_book(id: &str) -> Book {
    Book::new(
        id.to_string(),
        fingerprint(),
        Format::Markdown,
        Origin::Linked {
            src: format!("/books/{id}.md"),
        },
        0,
    )
}

pub fn markdown_row(id: &str) -> Row {
    Row::Book(markdown_book(id))
}

/// A book row at `path`, for the tests where the id and the address differ.
pub fn book_at(id: &str, path: &str) -> Book {
    Book {
        origin: Origin::Linked {
            src: path.to_string(),
        },
        ..book(id)
    }
}

pub fn row_at(id: &str, path: &str) -> Row {
    Row::Book(book_at(id, path))
}

/// A pointer row: a link called `name` at `target`, made at `0`.
pub fn link(id: &str, name: &str, target: &str) -> Row {
    Row::link(id.to_string(), name.to_string(), target.to_string(), 0)
}

/// A virtual shelf holding `members`, hanging under `parent` (`None` is the root).
pub fn shelf(id: &str, name: &str, members: &[&str], parent: Option<&str>) -> Shelf {
    Shelf {
        id: id.to_string(),
        name: name.to_string(),
        kind: ShelfKind::Virtual,
        books: members.iter().map(|m| m.to_string()).collect(),
        parent: parent.map(str::to_string),
        manual_parent: false,
    }
}

/// A shelf named by its id, where the name is not what the test is about.
pub fn plain_shelf(id: &str, members: &[&str]) -> Shelf {
    shelf(id, id, members, None)
}

/// A shelf a move took off its tree: the reader's own now, holding the copies that move paid for.
pub fn departed_shelf(id: &str, members: &[&str], parent: Option<&str>) -> Shelf {
    Shelf {
        kind: ShelfKind::Departed,
        ..shelf(id, id, members, parent)
    }
}

/// A folder shelf: one rung of a watched folder's tree, at `rel` (`None` is the root rung).
pub fn folder_shelf(
    id: &str,
    name: &str,
    folder_id: &str,
    rel: Option<&str>,
    members: &[&str],
    parent: Option<&str>,
) -> Shelf {
    Shelf {
        kind: ShelfKind::Folder {
            folder_id: folder_id.to_string(),
            rel: rel.map(str::to_string),
        },
        ..shelf(id, name, members, parent)
    }
}

/// A found Markdown file numbered `n`, whose `rel` is its own name.
pub fn found_md(path: &str, n: u32) -> FoundFile {
    FoundFile {
        rel: path.rsplit('/').next().unwrap_or(path).to_string(),
        path: path.to_string(),
        ext: "md".to_string(),
        size: u64::from(n),
        fp: fp_n(n),
    }
}
