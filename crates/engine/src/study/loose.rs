//! A document outside the library, known by its name for the session.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use mareader_library::{BookId, ContentId, Format, Library, Measure, Opened};

/// Records session document `name` in `loose`, where its marks are kept.
pub fn loose_book(loose: &mut Library, name: &str) -> BookId {
    let opened = Opened {
        path: PathBuf::from(format!("session:{name}")),
        measure: Measure {
            content: loose_content(name),
            stamp: Default::default(),
        },
        format: Format::Pdf,
        title: name.to_owned(),
        author: None,
    };
    loose.record_open(opened, 0)
}

/// Session document `name`'s id in a loose library.
pub fn loose_id(name: &str) -> BookId {
    BookId::original(loose_content(name))
}

/// A document outside the library, known by its name for the session.
pub fn loose_content(name: &str) -> ContentId {
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    ContentId(u128::from(h.finish()))
}
