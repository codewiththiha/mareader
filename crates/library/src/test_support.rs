//! Fixtures shared by the library's unit-test suites.

use std::path::PathBuf;

use crate::{ContentId, FileStamp, Format, Library, Measure, Opened, Position, Row};

pub(crate) fn opened(
    path: &str,
    content: u128,
    format: Format,
    stamp: Option<FileStamp>,
) -> Opened {
    Opened {
        path: PathBuf::from(path),
        measure: Measure {
            content: ContentId(content),
            stamp: stamp.unwrap_or_default(),
        },
        format,
        title: format!("Book {content}"),
        author: None,
    }
}

pub(crate) fn page(page: usize, pages: usize) -> Position {
    Position {
        page,
        pages,
        place: None,
        spine: None,
    }
}

/// The synced rows a machine hasn't sent yet.
pub(crate) fn unsent(lib: &mut Library) -> Vec<Row> {
    let dirty = lib.take_dirty();
    let rows = lib.changes(&dirty).put.into_iter();
    rows.filter(|r| r.kind.synced()).collect()
}

pub(crate) fn measure(content: u128) -> Measure {
    Measure {
        content: ContentId(content),
        stamp: FileStamp {
            size: 100,
            mtime_ms: 1,
        },
    }
}
